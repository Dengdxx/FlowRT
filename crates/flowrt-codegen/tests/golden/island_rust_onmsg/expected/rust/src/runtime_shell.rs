// FlowRT 管理产物。不要手工修改。

use crate::components::*;
use crate::messages::*;
use crate::selfdesc;
use crate::user;

const PACKAGE_NAME: &str = "island_rust_demo";

type FlowrtOutputCommit = Box<dyn FnOnce(&App, &flowrt::IntrospectionState, &flowrt::ScheduleWaiter, &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status + Send>;

pub struct App {
    startup_status: flowrt::Status,
    consumer: std::sync::Arc<std::sync::Mutex<Box<dyn Consumer + Send>>>,
    boundary_input_sample_in: flowrt::BoundaryInput<Sample>,
    boundary_output_echo_out: flowrt::BoundaryOutput<Sample>,
}

impl App {
    pub fn new(
        consumer: Box<dyn Consumer + Send>,
    ) -> Self {
        let startup_status = flowrt::Status::Ok;
        let consumer = std::sync::Arc::new(std::sync::Mutex::new(consumer));
        Self {
            consumer: consumer.clone(),
            boundary_input_sample_in: flowrt::BoundaryInput::new(),
            boundary_output_echo_out: flowrt::BoundaryOutput::new(),
            startup_status,
        }
    }
    #[allow(dead_code)]
    fn step(
        &self,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::Status {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        {
            let sample_read = self.boundary_input_sample_in.read_at(tick_time_ms);
            let __flowrt_sample_revision = sample_read.revision();
            let sample = sample_read.view();
            {
                let __h = health_map.entry("consumer.main".to_string()).or_default();
                __h.name = "consumer.main".to_string();
                __h.lane = "consumer_serial".to_string();
            }
            if sample.present() {
                let mut echo = flowrt::Output::<Sample>::new();
                match self.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(sample, &mut echo) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
                }
                if let Some(value) = echo.as_ref().cloned() {
                    self.boundary_output_echo_out.publish_at(&value, tick_time_ms);
                }
            }
        }
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_startup(
        &self,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::Status {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_shutdown(
        &self,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::Status {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_task_consumer_main(
        __flowrt_component_consumer: std::sync::Arc<std::sync::Mutex<Box<dyn Consumer + Send>>>,
        __flowrt_input_sample_value: Option<Sample>,
        __flowrt_input_sample_stale: bool,
        __flowrt_input_sample_revision: u64,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::TaskRunOutcome<Vec<FlowrtOutputCommit>> {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();
        {
            let sample = flowrt::Latest::new(__flowrt_input_sample_value.as_ref(), __flowrt_input_sample_stale);
            let __flowrt_sample_revision = __flowrt_input_sample_revision;
            {
                let __h = health_map.entry("consumer.main".to_string()).or_default();
                __h.name = "consumer.main".to_string();
                __h.lane = "consumer_serial".to_string();
            }
            if sample.present() {
                let mut echo = flowrt::Output::<Sample>::new();
                match __flowrt_component_consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(sample, &mut echo) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
                }
                if let Some(value) = echo.as_ref().cloned() {
                    let value = value.clone();
                    __flowrt_output_commits.push(Box::new(move |app, _introspection_state, _scheduler_events, _health_map| {
                        app.boundary_output_echo_out.publish_at(&value, tick_time_ms);
                        flowrt::Status::Ok
                    }));
                }
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_main(
        &self,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::Status {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        {
            let sample_read = self.boundary_input_sample_in.read_at(tick_time_ms);
            let __flowrt_sample_revision = sample_read.revision();
            let sample = sample_read.view();
            {
                let __h = health_map.entry("consumer.main".to_string()).or_default();
                __h.name = "consumer.main".to_string();
                __h.lane = "consumer_serial".to_string();
            }
            if sample.present() {
                let mut echo = flowrt::Output::<Sample>::new();
                match self.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(sample, &mut echo) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
                }
                if let Some(value) = echo.as_ref().cloned() {
                    self.boundary_output_echo_out.publish_at(&value, tick_time_ms);
                }
            }
        }
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_main_startup(
        &self,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::Status {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_main_shutdown(
        &self,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::Status {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_main_task_consumer_main(
        __flowrt_component_consumer: std::sync::Arc<std::sync::Mutex<Box<dyn Consumer + Send>>>,
        __flowrt_input_sample_value: Option<Sample>,
        __flowrt_input_sample_stale: bool,
        __flowrt_input_sample_revision: u64,
        tick: usize,
        _tick_context: &mut flowrt::Context,
        introspection_state: &flowrt::IntrospectionState,
        scheduler_events: &flowrt::ScheduleWaiter,
        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,
    ) -> flowrt::TaskRunOutcome<Vec<FlowrtOutputCommit>> {
        let _ = tick;
        let _ = introspection_state;
        let _ = scheduler_events;
        let _ = health_map;
        let tick_time_ms = tick as u64;
        let _ = tick_time_ms;
        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();
        {
            let sample = flowrt::Latest::new(__flowrt_input_sample_value.as_ref(), __flowrt_input_sample_stale);
            let __flowrt_sample_revision = __flowrt_input_sample_revision;
            {
                let __h = health_map.entry("consumer.main".to_string()).or_default();
                __h.name = "consumer.main".to_string();
                __h.lane = "consumer_serial".to_string();
            }
            if sample.present() {
                let mut echo = flowrt::Output::<Sample>::new();
                match __flowrt_component_consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(sample, &mut echo) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
                }
                if let Some(value) = echo.as_ref().cloned() {
                    let value = value.clone();
                    __flowrt_output_commits.push(Box::new(move |app, _introspection_state, _scheduler_events, _health_map| {
                        app.boundary_output_echo_out.publish_at(&value, tick_time_ms);
                        flowrt::Status::Ok
                    }));
                }
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    pub fn run(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {
        if self.startup_status != flowrt::Status::Ok {
            return self.startup_status;
        }
        let app = std::sync::Arc::new(self);
        let mut lifecycle_context = flowrt::Context::default();
        let mut status = flowrt::Status::Ok;
        let _ = backend;
        let shutdown = flowrt::install_signal_shutdown_token();
        let introspection_state = flowrt::IntrospectionState::new();
        let scheduler_events = flowrt::ScheduleWaiter::new();
        app.boundary_input_sample_in.set_schedule_waiter(scheduler_events.clone());
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        introspection_state.register_boundary_input::<Sample>("sample_in", "Sample", app.boundary_input_sample_in.clone());
        introspection_state.register_channel("echo_out", "Sample");
        let _boundary_output_echo_out_probe = app.boundary_output_echo_out.register_sink({
            let introspection_state = introspection_state.clone();
            move |value, published_at_ms| {
                let mut payload = vec![0u8; <Sample as flowrt::FrameCodec>::encoded_frame_size(value)];
                if <Sample as flowrt::FrameCodec>::encode_frame(value, &mut payload).is_ok() {
                    introspection_state.record_channel_publish_bytes("echo_out", "Sample", payload, published_at_ms);
                }
            }
        });
        let _introspection_server = flowrt::spawn_status_server(
            flowrt::IntrospectionIdentity {
                self_description_hash: selfdesc::self_description_hash().to_string(),
                package: PACKAGE_NAME.to_string(),
                process: "main".to_string(),
                runtime: "rust".to_string(),
            },
            introspection_state.clone(),
        )
        .ok();
        let mut consumer_initialized = false;
        let mut consumer_started = false;
        if status == flowrt::Status::Ok {
            status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            consumer_initialized = status == flowrt::Status::Ok;
        }
        if status == flowrt::Status::Ok && consumer_initialized {
            status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            consumer_started = status == flowrt::Status::Ok;
        }
        if status == flowrt::Status::Ok {
            status = app.step_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "consumer_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        let mut boundary_input_sample_in_seen_revision_for_consumer_main: u64 = 0;
        let scheduler_base_period_ms: u64 = 1;
        let mut tick_base: usize = 0;
        let mut scheduler_now_ms: u64 = 0;
        let mut health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
        const FAIRNESS_STARVATION_THRESHOLD: u64 = 10;
        let scheduler_started_at = std::time::Instant::now();
        let scheduler_runtime_now_ms = || -> u64 {
            scheduler_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        };
        let clock_source = "realtime";
        let task_clock_source = flowrt::ClockSource::Runtime;
        let task_completion_queue = flowrt::WorkerCompletionQueue::<Vec<FlowrtOutputCommit>>::new();
        let scheduler_events_for_task_completion = scheduler_events.clone();
        task_completion_queue.set_wake_callback(move || scheduler_events_for_task_completion.notify_data());
        let mut pending_task_order: std::collections::VecDeque<flowrt::TaskId> = std::collections::VecDeque::new();
        let mut pending_task_results: std::collections::BTreeMap<flowrt::TaskId, flowrt::TaskRunOutput<Vec<FlowrtOutputCommit>>> = std::collections::BTreeMap::new();
        let mut pending_task_admissions: std::collections::BTreeMap<flowrt::TaskId, flowrt::TaskAdmission> = std::collections::BTreeMap::new();
        let task_health_from_workers = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::<String, flowrt::IntrospectionTaskHealth>::new()));
        let mut task_last_scheduled_time_ms: std::collections::BTreeMap<flowrt::TaskId, u64> = std::collections::BTreeMap::new();
        let mut task_last_observed_time_ms: std::collections::BTreeMap<flowrt::TaskId, u64> = std::collections::BTreeMap::new();
        while status == flowrt::Status::Ok
            && !shutdown.is_requested()
            && (run_ticks
                .map(|limit| tick_base < limit)
                .unwrap_or(true)
                || !pending_task_order.is_empty())
        {
            let mut observed_data_generation: u64;
            scheduler_now_ms = scheduler_now_ms.max(scheduler_runtime_now_ms());
            let _ = scheduler_events.take_data_time_ms();
            let tick_time_ms = scheduler_now_ms;
            scheduler.advance_to_ms(tick_time_ms);
            scheduler.set_current_tick(tick_base as u64);
            {
                let __h = health_map.entry("consumer.main".to_string()).or_default();
                __h.name = "consumer.main".to_string();
                __h.lane = "consumer_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                if app.boundary_input_sample_in.revision() != boundary_input_sample_in_seen_revision_for_consumer_main {
                    boundary_input_sample_in_seen_revision_for_consumer_main = app.boundary_input_sample_in.revision();
                    scheduler.wake(flowrt::TaskId(1));
                    woke_on_message = true;
                }
                for task_result in task_completion_queue.drain_completed() {
                    pending_task_results.insert(task_result.task, task_result);
                }
                {
                    let mut completed_health = task_health_from_workers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    health_map.append(&mut *completed_health);
                }
                let ready_batch = scheduler.take_ready_batch();
                let submitted_task_count = ready_batch.len();
                for admission in ready_batch.admissions().iter().copied() {
                    let scheduled_delta_ms = task_last_scheduled_time_ms
                        .insert(admission.task, admission.scheduled_time_ms)
                        .map_or(0, |last| admission.scheduled_time_ms.saturating_sub(last));
                    let observed_delta_ms = task_last_observed_time_ms
                        .insert(admission.task, admission.observed_time_ms)
                        .map_or(0, |last| admission.observed_time_ms.saturating_sub(last));
                    let task_completion_queue_for_task = task_completion_queue.clone();
                    let submitted = match admission.task {
                        flowrt::TaskId(1) => {
                            let __flowrt_component_consumer = app.consumer.clone();
                            let (__flowrt_input_sample_value, __flowrt_input_sample_stale, __flowrt_input_sample_revision) = {
                                let __flowrt_sample_boundary_read = app.boundary_input_sample_in.read_at(tick_time_ms);
                                let __flowrt_sample_snapshot_view = __flowrt_sample_boundary_read.view();
                                (__flowrt_sample_snapshot_view.as_ref().cloned(), __flowrt_sample_snapshot_view.stale(), __flowrt_sample_boundary_read.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "consumer.main";
                            let task_trigger = "on_message";
                            let mut local_context = flowrt::Context::with_timing(flowrt::TaskTiming {
                                step: tick_base as u64,
                                task_name: task_name.to_string(),
                                trigger: task_trigger.to_string(),
                                clock_source: task_clock_source,
                                scheduled_time_ms: admission.scheduled_time_ms,
                                observed_time_ms: admission.observed_time_ms,
                                scheduled_delta_ms,
                                observed_delta_ms,
                                period_ms: admission.period_ms,
                                deadline_ms: admission.deadline_ms,
                                lateness_ms: admission.lateness_ms,
                                missed_periods: admission.missed_periods,
                                deadline_missed: admission.deadline_ms.map_or(false, |deadline_ms| admission.lateness_ms > deadline_ms),
                                overrun: admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms),
                            });
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = {
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(1));
                                Self::step_task_consumer_main(__flowrt_component_consumer, __flowrt_input_sample_value, __flowrt_input_sample_stale, __flowrt_input_sample_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            {
                                let mut merged_health = task_health_from_worker.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                for (name, health) in local_health_map {
                                    merged_health.insert(name, health);
                                }
                            }
                            task_outcome
                            })
                        },
                        _ => {
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "__flowrt_hidden";
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = flowrt::TaskRunOutcome::error(Vec::new());
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            {
                                let mut merged_health = task_health_from_worker.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                for (name, health) in local_health_map {
                                    merged_health.insert(name, health);
                                }
                            }
                            task_outcome
                            })
                        },
                    };
                    match submitted {
                        Ok(()) => {
                            pending_task_order.push_back(admission.task);
                            pending_task_admissions.insert(admission.task, admission);
                            match admission.task {
                                flowrt::TaskId(1) => {
                                    let health = health_map.entry("consumer.main".to_string()).or_default();
                                    health.name = "consumer.main".to_string();
                                    health.lane = "consumer_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                _ => {}
                            }
                        }
                        Err(_) => {
                            let _ = scheduler.complete_task(admission.task);
                            status = flowrt::Status::Error;
                            break;
                        }
                    }
                }
                if status != flowrt::Status::Ok {
                    break;
                }
                let mut committed_task_count = 0usize;
                while let Some(task) = pending_task_order.front().copied() {
                    let Some(task_result) = pending_task_results.remove(&task) else {
                        break;
                    };
                    pending_task_order.pop_front();
                    let _ = scheduler.complete_task(task_result.task);
                    committed_task_count += 1;
                    match task_result.task {
                        flowrt::TaskId(1) => {
                            let health = health_map.entry("consumer.main".to_string()).or_default();
                            health.name = "consumer.main".to_string();
                            health.lane = "consumer_serial".to_string();
                            health.inflight = false;
                            if let Some(admission) = pending_task_admissions.remove(&task_result.task) {
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            health.run_count += 1;
                            health.last_run_ms = Some(tick_time_ms);
                            if task_result.status == flowrt::Status::Ok {
                                health.success_count += 1;
                                health.consecutive_failures = 0;
                                health.last_success_ms = Some(tick_time_ms);
                            } else if task_result.status == flowrt::Status::Error {
                                health.consecutive_failures += 1;
                            }
                        }
                        _ => {}
                    }
                    if task_result.status == flowrt::Status::Error {
                        status = flowrt::Status::Error;
                        break;
                    }
                    if let Some(commits) = task_result.outputs {
                        for commit in commits {
                            let commit_status = commit(app.as_ref(), &introspection_state, &scheduler_events, &mut health_map);
                            if commit_status == flowrt::Status::Error {
                                status = flowrt::Status::Error;
                                break;
                            }
                            if commit_status == flowrt::Status::Retry {
                                status = flowrt::Status::Retry;
                                break;
                            }
                        }
                    }
                    if status != flowrt::Status::Ok {
                        break;
                    }
                }
                if status != flowrt::Status::Ok {
                    break;
                }
                if committed_task_count == 0 || (!woke_on_message && submitted_task_count == 0) {
                    break;
                }
            }
            // 公平性检测：检查 lane 饥饿。
            if scheduler.lane_starvation_ticks(flowrt::LaneId(1)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "consumer_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            // 将本轮健康快照写入 introspection。
            for (_, health) in health_map.iter_mut() {
                introspection_state.record_task_health(health.clone());
            }
            health_map.clear();
            if status == flowrt::Status::Ok {
                tick_base += 1;
                if run_ticks.is_some() && pending_task_order.is_empty() {
                    scheduler_now_ms = scheduler_now_ms.saturating_add(scheduler_base_period_ms);
                    continue;
                }
                let next_periodic_deadline_ms = None::<u64>;
                let next_wake_deadline = next_periodic_deadline_ms.map(|deadline_ms| {
                    std::time::Instant::now()
                        + std::time::Duration::from_millis(deadline_ms.saturating_sub(scheduler_now_ms))
                });
                match scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown) {
                    flowrt::ScheduleEvent::Shutdown => break,
                    flowrt::ScheduleEvent::Timer => {
                        scheduler_now_ms = next_periodic_deadline_ms
                            .unwrap_or_else(|| scheduler_now_ms.saturating_add(scheduler_base_period_ms));
                    }
                    flowrt::ScheduleEvent::Data => {
                        scheduler_now_ms = scheduler_now_ms.max(scheduler_runtime_now_ms());
                        let _ = scheduler_events.take_data_time_ms();
                    }
                }
            }
        }
        if status == flowrt::Status::Ok {
            status = app.step_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if consumer_started {
            let stop_status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
        }
        if consumer_initialized {
            let shutdown_status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
        }
        status
    }
    pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str, run_ticks: Option<usize>) -> flowrt::Status {
        match process {
            "main" => self.run_process_main(backend, run_ticks),
            _ => flowrt::Status::Error,
        }
    }
    fn run_process_main(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {
        if self.startup_status != flowrt::Status::Ok {
            return self.startup_status;
        }
        let app = std::sync::Arc::new(self);
        let mut lifecycle_context = flowrt::Context::default();
        let mut status = flowrt::Status::Ok;
        let _ = backend;
        let shutdown = flowrt::install_signal_shutdown_token();
        let introspection_state = flowrt::IntrospectionState::new();
        let scheduler_events = flowrt::ScheduleWaiter::new();
        app.boundary_input_sample_in.set_schedule_waiter(scheduler_events.clone());
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        introspection_state.register_boundary_input::<Sample>("sample_in", "Sample", app.boundary_input_sample_in.clone());
        introspection_state.register_channel("echo_out", "Sample");
        let _boundary_output_echo_out_probe = app.boundary_output_echo_out.register_sink({
            let introspection_state = introspection_state.clone();
            move |value, published_at_ms| {
                let mut payload = vec![0u8; <Sample as flowrt::FrameCodec>::encoded_frame_size(value)];
                if <Sample as flowrt::FrameCodec>::encode_frame(value, &mut payload).is_ok() {
                    introspection_state.record_channel_publish_bytes("echo_out", "Sample", payload, published_at_ms);
                }
            }
        });
        let _introspection_server = flowrt::spawn_status_server(
            flowrt::IntrospectionIdentity {
                self_description_hash: selfdesc::self_description_hash().to_string(),
                package: PACKAGE_NAME.to_string(),
                process: "main".to_string(),
                runtime: "rust".to_string(),
            },
            introspection_state.clone(),
        )
        .ok();
        let mut consumer_initialized = false;
        let mut consumer_started = false;
        if status == flowrt::Status::Ok {
            status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            consumer_initialized = status == flowrt::Status::Ok;
        }
        if status == flowrt::Status::Ok && consumer_initialized {
            status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            consumer_started = status == flowrt::Status::Ok;
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_main_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "consumer_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        let mut boundary_input_sample_in_seen_revision_for_consumer_main: u64 = 0;
        let scheduler_base_period_ms: u64 = 1;
        let mut tick_base: usize = 0;
        let mut scheduler_now_ms: u64 = 0;
        let mut health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
        const FAIRNESS_STARVATION_THRESHOLD: u64 = 10;
        let scheduler_started_at = std::time::Instant::now();
        let scheduler_runtime_now_ms = || -> u64 {
            scheduler_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        };
        let clock_source = "realtime";
        let task_clock_source = flowrt::ClockSource::Runtime;
        let task_completion_queue = flowrt::WorkerCompletionQueue::<Vec<FlowrtOutputCommit>>::new();
        let scheduler_events_for_task_completion = scheduler_events.clone();
        task_completion_queue.set_wake_callback(move || scheduler_events_for_task_completion.notify_data());
        let mut pending_task_order: std::collections::VecDeque<flowrt::TaskId> = std::collections::VecDeque::new();
        let mut pending_task_results: std::collections::BTreeMap<flowrt::TaskId, flowrt::TaskRunOutput<Vec<FlowrtOutputCommit>>> = std::collections::BTreeMap::new();
        let mut pending_task_admissions: std::collections::BTreeMap<flowrt::TaskId, flowrt::TaskAdmission> = std::collections::BTreeMap::new();
        let task_health_from_workers = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::<String, flowrt::IntrospectionTaskHealth>::new()));
        let mut task_last_scheduled_time_ms: std::collections::BTreeMap<flowrt::TaskId, u64> = std::collections::BTreeMap::new();
        let mut task_last_observed_time_ms: std::collections::BTreeMap<flowrt::TaskId, u64> = std::collections::BTreeMap::new();
        while status == flowrt::Status::Ok
            && !shutdown.is_requested()
            && (run_ticks
                .map(|limit| tick_base < limit)
                .unwrap_or(true)
                || !pending_task_order.is_empty())
        {
            let mut observed_data_generation: u64;
            scheduler_now_ms = scheduler_now_ms.max(scheduler_runtime_now_ms());
            let _ = scheduler_events.take_data_time_ms();
            let tick_time_ms = scheduler_now_ms;
            scheduler.advance_to_ms(tick_time_ms);
            scheduler.set_current_tick(tick_base as u64);
            {
                let __h = health_map.entry("consumer.main".to_string()).or_default();
                __h.name = "consumer.main".to_string();
                __h.lane = "consumer_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                if app.boundary_input_sample_in.revision() != boundary_input_sample_in_seen_revision_for_consumer_main {
                    boundary_input_sample_in_seen_revision_for_consumer_main = app.boundary_input_sample_in.revision();
                    scheduler.wake(flowrt::TaskId(1));
                    woke_on_message = true;
                }
                for task_result in task_completion_queue.drain_completed() {
                    pending_task_results.insert(task_result.task, task_result);
                }
                {
                    let mut completed_health = task_health_from_workers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    health_map.append(&mut *completed_health);
                }
                let ready_batch = scheduler.take_ready_batch();
                let submitted_task_count = ready_batch.len();
                for admission in ready_batch.admissions().iter().copied() {
                    let scheduled_delta_ms = task_last_scheduled_time_ms
                        .insert(admission.task, admission.scheduled_time_ms)
                        .map_or(0, |last| admission.scheduled_time_ms.saturating_sub(last));
                    let observed_delta_ms = task_last_observed_time_ms
                        .insert(admission.task, admission.observed_time_ms)
                        .map_or(0, |last| admission.observed_time_ms.saturating_sub(last));
                    let task_completion_queue_for_task = task_completion_queue.clone();
                    let submitted = match admission.task {
                        flowrt::TaskId(1) => {
                            let __flowrt_component_consumer = app.consumer.clone();
                            let (__flowrt_input_sample_value, __flowrt_input_sample_stale, __flowrt_input_sample_revision) = {
                                let __flowrt_sample_boundary_read = app.boundary_input_sample_in.read_at(tick_time_ms);
                                let __flowrt_sample_snapshot_view = __flowrt_sample_boundary_read.view();
                                (__flowrt_sample_snapshot_view.as_ref().cloned(), __flowrt_sample_snapshot_view.stale(), __flowrt_sample_boundary_read.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "consumer.main";
                            let task_trigger = "on_message";
                            let mut local_context = flowrt::Context::with_timing(flowrt::TaskTiming {
                                step: tick_base as u64,
                                task_name: task_name.to_string(),
                                trigger: task_trigger.to_string(),
                                clock_source: task_clock_source,
                                scheduled_time_ms: admission.scheduled_time_ms,
                                observed_time_ms: admission.observed_time_ms,
                                scheduled_delta_ms,
                                observed_delta_ms,
                                period_ms: admission.period_ms,
                                deadline_ms: admission.deadline_ms,
                                lateness_ms: admission.lateness_ms,
                                missed_periods: admission.missed_periods,
                                deadline_missed: admission.deadline_ms.map_or(false, |deadline_ms| admission.lateness_ms > deadline_ms),
                                overrun: admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms),
                            });
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = {
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(1));
                                Self::step_process_main_task_consumer_main(__flowrt_component_consumer, __flowrt_input_sample_value, __flowrt_input_sample_stale, __flowrt_input_sample_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            {
                                let mut merged_health = task_health_from_worker.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                for (name, health) in local_health_map {
                                    merged_health.insert(name, health);
                                }
                            }
                            task_outcome
                            })
                        },
                        _ => {
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "__flowrt_hidden";
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = flowrt::TaskRunOutcome::error(Vec::new());
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            {
                                let mut merged_health = task_health_from_worker.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                for (name, health) in local_health_map {
                                    merged_health.insert(name, health);
                                }
                            }
                            task_outcome
                            })
                        },
                    };
                    match submitted {
                        Ok(()) => {
                            pending_task_order.push_back(admission.task);
                            pending_task_admissions.insert(admission.task, admission);
                            match admission.task {
                                flowrt::TaskId(1) => {
                                    let health = health_map.entry("consumer.main".to_string()).or_default();
                                    health.name = "consumer.main".to_string();
                                    health.lane = "consumer_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                _ => {}
                            }
                        }
                        Err(_) => {
                            let _ = scheduler.complete_task(admission.task);
                            status = flowrt::Status::Error;
                            break;
                        }
                    }
                }
                if status != flowrt::Status::Ok {
                    break;
                }
                let mut committed_task_count = 0usize;
                while let Some(task) = pending_task_order.front().copied() {
                    let Some(task_result) = pending_task_results.remove(&task) else {
                        break;
                    };
                    pending_task_order.pop_front();
                    let _ = scheduler.complete_task(task_result.task);
                    committed_task_count += 1;
                    match task_result.task {
                        flowrt::TaskId(1) => {
                            let health = health_map.entry("consumer.main".to_string()).or_default();
                            health.name = "consumer.main".to_string();
                            health.lane = "consumer_serial".to_string();
                            health.inflight = false;
                            if let Some(admission) = pending_task_admissions.remove(&task_result.task) {
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            health.run_count += 1;
                            health.last_run_ms = Some(tick_time_ms);
                            if task_result.status == flowrt::Status::Ok {
                                health.success_count += 1;
                                health.consecutive_failures = 0;
                                health.last_success_ms = Some(tick_time_ms);
                            } else if task_result.status == flowrt::Status::Error {
                                health.consecutive_failures += 1;
                            }
                        }
                        _ => {}
                    }
                    if task_result.status == flowrt::Status::Error {
                        status = flowrt::Status::Error;
                        break;
                    }
                    if let Some(commits) = task_result.outputs {
                        for commit in commits {
                            let commit_status = commit(app.as_ref(), &introspection_state, &scheduler_events, &mut health_map);
                            if commit_status == flowrt::Status::Error {
                                status = flowrt::Status::Error;
                                break;
                            }
                            if commit_status == flowrt::Status::Retry {
                                status = flowrt::Status::Retry;
                                break;
                            }
                        }
                    }
                    if status != flowrt::Status::Ok {
                        break;
                    }
                }
                if status != flowrt::Status::Ok {
                    break;
                }
                if committed_task_count == 0 || (!woke_on_message && submitted_task_count == 0) {
                    break;
                }
            }
            // 公平性检测：检查 lane 饥饿。
            if scheduler.lane_starvation_ticks(flowrt::LaneId(1)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "consumer_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            // 将本轮健康快照写入 introspection。
            for (_, health) in health_map.iter_mut() {
                introspection_state.record_task_health(health.clone());
            }
            health_map.clear();
            if status == flowrt::Status::Ok {
                tick_base += 1;
                if run_ticks.is_some() && pending_task_order.is_empty() {
                    scheduler_now_ms = scheduler_now_ms.saturating_add(scheduler_base_period_ms);
                    continue;
                }
                let next_periodic_deadline_ms = None::<u64>;
                let next_wake_deadline = next_periodic_deadline_ms.map(|deadline_ms| {
                    std::time::Instant::now()
                        + std::time::Duration::from_millis(deadline_ms.saturating_sub(scheduler_now_ms))
                });
                match scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown) {
                    flowrt::ScheduleEvent::Shutdown => break,
                    flowrt::ScheduleEvent::Timer => {
                        scheduler_now_ms = next_periodic_deadline_ms
                            .unwrap_or_else(|| scheduler_now_ms.saturating_add(scheduler_base_period_ms));
                    }
                    flowrt::ScheduleEvent::Data => {
                        scheduler_now_ms = scheduler_now_ms.max(scheduler_runtime_now_ms());
                        let _ = scheduler_events.take_data_time_ms();
                    }
                }
            }
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_main_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if consumer_started {
            let stop_status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
        }
        if consumer_initialized {
            let shutdown_status = app.consumer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
        }
        status
    }
}

pub fn backend() -> Box<dyn flowrt::Backend> {
    Box::new(flowrt::inproc_backend())
}

pub fn run(run_ticks: Option<usize>) -> flowrt::Status {
    let backend = backend();
    user::build_app().run(backend.as_ref(), run_ticks)
}

pub fn run_process(process: &str, run_ticks: Option<usize>) -> flowrt::Status {
    let backend = backend();
    user::build_app().run_process(backend.as_ref(), process, run_ticks)
}
