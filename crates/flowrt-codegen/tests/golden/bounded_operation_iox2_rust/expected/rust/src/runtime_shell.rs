// FlowRT 管理产物。不要手工修改。

use crate::components::*;
use crate::messages::*;
use crate::selfdesc;
use crate::user;

const PACKAGE_NAME: &str = "bounded_operation_iox2_rust";

type FlowrtOutputCommit = Box<dyn FnOnce(&App, &flowrt::IntrospectionState, &flowrt::ScheduleWaiter, &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status + Send>;

pub struct App {
    startup_status: flowrt::Status,
    controller: std::sync::Arc<std::sync::Mutex<Box<dyn Controller + Send>>>,
    navigator: std::sync::Arc<std::sync::Mutex<Box<dyn Navigator + Send>>>,
    operation_client_controller_plan: OperationClient_controller_plan,
    operation_control_0: std::sync::Arc<std::sync::Mutex<flowrt::OperationControl>>,
    operation_start_server_navigator_plan: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<flowrt::iox2::Iox2FrameServiceServer<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>>>>,
operation_cancel_server_navigator_plan: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>>,
operation_status_server_navigator_plan: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>>,
}

impl App {
    pub fn new(
        controller: Box<dyn Controller + Send>,
        navigator: Box<dyn Navigator + Send>,
    ) -> Self {
        let startup_status = flowrt::Status::Ok;
        let controller = std::sync::Arc::new(std::sync::Mutex::new(controller));
        let navigator = std::sync::Arc::new(std::sync::Mutex::new(navigator));
        let operation_policy_0 = match flowrt::OperationPolicy::new(
std::time::Duration::from_millis(5000),
flowrt::OperationConcurrencyPolicy::Reject,
flowrt::OperationPreemptPolicy::Reject,
4,
1,
).and_then(|policy| policy.with_result_retention(std::time::Duration::from_millis(60000))) {
Ok(policy) => policy,
Err(error) => panic!("validated operation policy rejected at runtime: {error}"),
};
let operation_control_0 = std::sync::Arc::new(std::sync::Mutex::new(flowrt::OperationControl::new(flowrt::fnv1a64("controller.plan".as_bytes()), operation_policy_0)));
        Self {
            controller: controller.clone(),
            navigator: navigator.clone(),
            operation_client_controller_plan: OperationClient_controller_plan { start_client: std::sync::Arc::new(std::sync::OnceLock::new()), cancel_client: std::sync::Arc::new(std::sync::OnceLock::new()), status_client: std::sync::Arc::new(std::sync::OnceLock::new()) },
operation_control_0: operation_control_0.clone(),
            operation_start_server_navigator_plan: std::sync::Arc::new(std::sync::OnceLock::new()),
operation_cancel_server_navigator_plan: std::sync::Arc::new(std::sync::OnceLock::new()),
operation_status_server_navigator_plan: std::sync::Arc::new(std::sync::OnceLock::new()),
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
        {
            {
                let __h = health_map.entry("controller.main".to_string()).or_default();
                __h.name = "controller.main".to_string();
                __h.lane = "controller_serial".to_string();
            }
            match self.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&self.operation_client_controller_plan) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
        }
        {
            {
                let __h = health_map.entry("navigator.main".to_string()).or_default();
                __h.name = "navigator.main".to_string();
                __h.lane = "navigator_serial".to_string();
            }
            match self.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick() {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
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
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_task_controller_main(
        __flowrt_component_controller: std::sync::Arc<std::sync::Mutex<Box<dyn Controller + Send>>>,
        __flowrt_operation_client_controller_plan: OperationClient_controller_plan,
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
        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();
        {
            {
                let __h = health_map.entry("controller.main".to_string()).or_default();
                __h.name = "controller.main".to_string();
                __h.lane = "controller_serial".to_string();
            }
            match __flowrt_component_controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&__flowrt_operation_client_controller_plan) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_task_navigator_main(
        __flowrt_component_navigator: std::sync::Arc<std::sync::Mutex<Box<dyn Navigator + Send>>>,
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
        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();
        {
            {
                let __h = health_map.entry("navigator.main".to_string()).or_default();
                __h.name = "navigator.main".to_string();
                __h.lane = "navigator_serial".to_string();
            }
            match __flowrt_component_navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick() {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_client_proc(
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
        {
            {
                let __h = health_map.entry("controller.main".to_string()).or_default();
                __h.name = "controller.main".to_string();
                __h.lane = "controller_serial".to_string();
            }
            match self.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&self.operation_client_controller_plan) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
        }
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_client_proc_startup(
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
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_client_proc_shutdown(
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
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_client_proc_task_controller_main(
        __flowrt_component_controller: std::sync::Arc<std::sync::Mutex<Box<dyn Controller + Send>>>,
        __flowrt_operation_client_controller_plan: OperationClient_controller_plan,
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
        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();
        {
            {
                let __h = health_map.entry("controller.main".to_string()).or_default();
                __h.name = "controller.main".to_string();
                __h.lane = "controller_serial".to_string();
            }
            match __flowrt_component_controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&__flowrt_operation_client_controller_plan) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_server_proc(
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
        {
            {
                let __h = health_map.entry("navigator.main".to_string()).or_default();
                __h.name = "navigator.main".to_string();
                __h.lane = "navigator_serial".to_string();
            }
            match self.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick() {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
        }
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_server_proc_startup(
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
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_server_proc_shutdown(
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
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_server_proc_task_navigator_main(
        __flowrt_component_navigator: std::sync::Arc<std::sync::Mutex<Box<dyn Navigator + Send>>>,
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
        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();
        {
            {
                let __h = health_map.entry("navigator.main".to_string()).or_default();
                __h.name = "navigator.main".to_string();
                __h.lane = "navigator_serial".to_string();
            }
            match __flowrt_component_navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick() {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
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
        introspection_state.set_self_description_json(selfdesc::self_description_json());
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
        let mut controller_initialized = false;
        let mut controller_started = false;
        introspection_state.record_lifecycle_state("controller", flowrt::LifecycleState::Uninitialized);
        let mut navigator_initialized = false;
        let mut navigator_started = false;
        introspection_state.record_lifecycle_state("navigator", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            controller_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("controller", if controller_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            navigator_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("navigator", if navigator_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && controller_initialized {
            status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            controller_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("controller", if controller_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && navigator_initialized {
            status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            navigator_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("navigator", if navigator_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if status == flowrt::Status::Ok {
        let _ = app.operation_client_controller_plan.start_client.set(match flowrt::iox2::Iox2FrameServiceClient::<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start") {
            Ok(client) => client,
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation start client {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_start");
                status = flowrt::Status::Error;
                flowrt::iox2::Iox2FrameServiceClient::<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::unavailable("FlowRT/service/__flowrt_operation_controller_plan_start", error.to_string())
            }
        });
        let _ = app.operation_client_controller_plan.cancel_client.set(match flowrt::iox2::Iox2ServiceClient::open("FlowRT/service/__flowrt_operation_controller_plan_cancel") {
            Ok(client) => client,
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation cancel client {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_cancel");
                status = flowrt::Status::Error;
                flowrt::iox2::Iox2ServiceClient::unavailable("FlowRT/service/__flowrt_operation_controller_plan_cancel", error.to_string())
            }
        });
        let _ = app.operation_client_controller_plan.status_client.set(match flowrt::iox2::Iox2ServiceClient::open("FlowRT/service/__flowrt_operation_controller_plan_status") {
            Ok(client) => client,
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation status client {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_status");
                status = flowrt::Status::Error;
                flowrt::iox2::Iox2ServiceClient::unavailable("FlowRT/service/__flowrt_operation_controller_plan_status", error.to_string())
            }
        });
        match flowrt::iox2::Iox2FrameServiceServer::<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start", 1usize) {
            Ok(mut server) => {
                server.set_schedule_waiter(scheduler_events.clone());
                let _ = app.operation_start_server_navigator_plan.set(std::sync::Mutex::new(server));
            }
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation start server {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_start");
                status = flowrt::Status::Error;
            }
        }
        match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_cancel", 1usize) {
            Ok(mut server) => {
                server.set_schedule_waiter(scheduler_events.clone());
                let _ = app.operation_cancel_server_navigator_plan.set(std::sync::Mutex::new(server));
            }
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation cancel server {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_cancel");
                status = flowrt::Status::Error;
            }
        }
        match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_status", 1usize) {
            Ok(mut server) => {
                server.set_schedule_waiter(scheduler_events.clone());
                let _ = app.operation_status_server_navigator_plan.set(std::sync::Mutex::new(server));
            }
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation status server {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_status");
                status = flowrt::Status::Error;
            }
        }
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "controller_serial";
        scheduler.add_lane(flowrt::LaneId(2), flowrt::LaneKind::Serial);
        let _ = "navigator_serial";
        scheduler.add_lane(flowrt::LaneId(3), flowrt::LaneKind::Serial);
        let _ = "navigator_operation_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(1), period_ms: 100 });
        scheduler.wake(flowrt::TaskId(1));
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(2), period_ms: 1000 });
        scheduler.wake(flowrt::TaskId(2));
        // Operation task 3: controller.plan
scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(3), lane: flowrt::LaneId(3), priority: 0 });
        let scheduler_base_period_ms: u64 = 100;
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
                let __h = health_map.entry("controller.main".to_string()).or_default();
                __h.name = "controller.main".to_string();
                __h.lane = "controller_serial".to_string();
            }
            {
                let __h = health_map.entry("navigator.main".to_string()).or_default();
                __h.name = "navigator.main".to_string();
                __h.lane = "navigator_serial".to_string();
            }
            let mut flowrt_operation_tick_driven_0 = false;
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                    let flowrt_operation_snapshot_0 = app.operation_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot();
    let flowrt_operation_active_0 = !flowrt_operation_snapshot_0.state.is_terminal()
    && flowrt_operation_snapshot_0.state != flowrt::OperationState::Idle;
    if (app.operation_start_server_navigator_plan.get().is_some()
                         || app.operation_cancel_server_navigator_plan.get().is_some()
                         || app.operation_status_server_navigator_plan.get().is_some()) && !flowrt_operation_tick_driven_0
                         || flowrt_operation_active_0 && !flowrt_operation_tick_driven_0 {
    scheduler.wake(flowrt::TaskId(3));
    flowrt_operation_tick_driven_0 = true;
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
                            let __flowrt_component_controller = app.controller.clone();
                            let __flowrt_operation_client_controller_plan = app.operation_client_controller_plan.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_name = "controller.main";
                            let task_trigger = "periodic";
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
                                Self::step_task_controller_main(__flowrt_component_controller, __flowrt_operation_client_controller_plan, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            for (name, health) in local_health_map {
                                health_map.insert(name, health);
                            }
                            pending_task_results.insert(admission.task, flowrt::TaskRunOutput::from_outcome(admission.task, task_outcome));
                            Ok(())
                        },
                        flowrt::TaskId(2) => {
                            let __flowrt_component_navigator = app.navigator.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "navigator.main";
                            let task_trigger = "periodic";
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
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(2));
                                Self::step_task_navigator_main(__flowrt_component_navigator, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                        flowrt::TaskId(3) => {
                            let __flowrt_operation_start_server_navigator_plan = app.operation_start_server_navigator_plan.clone();
                            let __flowrt_operation_cancel_server_navigator_plan = app.operation_cancel_server_navigator_plan.clone();
                            let __flowrt_operation_status_server_navigator_plan = app.operation_status_server_navigator_plan.clone();
                            let __flowrt_operation_server_0 = app.navigator.clone();
                            let __flowrt_operation_control_0 = app.operation_control_0.clone();
                            let introspection_state = introspection_state.clone();
                            let task_name = "__flowrt_operation.controller.plan";
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = 'flowrt_task: {
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(3));
                                let operation_cancel_control = __flowrt_operation_control_0.clone();
                                introspection_state.register_operation_cancel_handler("controller.plan", move |operation_id| {
                                    let mut control = operation_cancel_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let snapshot = control.snapshot();
                                    if flowrt_operation_id_string(snapshot.id) != operation_id {
                                        return Err(format!("stale operation invocation `{}`; current is `{}`", operation_id, flowrt_operation_id_string(snapshot.id)));
                                    }
                                    control.request_cancel(snapshot.id, snapshot.owner).map_err(|error| error.to_string())?;
                                    Ok(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", control.snapshot()))
                                });
                                if let Some(start_server) = __flowrt_operation_start_server_navigator_plan.get() {
                                    let mut start_server = start_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_start_control_0 = __flowrt_operation_control_0.clone();
                                    let operation_server_0 = __flowrt_operation_server_0.clone();
                                    if start_server.poll_requests(move |request: flowrt::OperationStartRequest<PlanGoal>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {
                                        let ack = match operation_start_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout()) {
                                            Ok(ack) => ack,
                                            Err(error) => return flowrt_operation_control_error(error),
                                        };
                                        let id = ack.id;
                                        let operation_worker_server = operation_server_0.clone();
                                        let operation_worker_control = operation_start_control_0.clone();
                                        let goal_for_worker = request.goal;
                                        let spawn_result = std::thread::Builder::new()
                                            .name("flowrt-operation-0".to_string())
                                            .spawn(move || {
                                                loop {
                                                    let should_start = {
                                                        let control = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        let status = match control.status(id) {
                                                            Ok(status) => status,
                                                            Err(_) => return,
                                                        };
                                                        if status.state.is_terminal() {
                                                            return;
                                                        }
                                                        control.ready_to_run(id)
                                                    };
                                                    if should_start {
                                                        break;
                                                    }
                                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                                }
                                                let cancel = match operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token_for(id) {
                                                    Some(cancel) => cancel,
                                                    None => return,
                                                };
                                                if operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id).is_err() {
                                                    return;
                                                }
                                                let operation_progress_control = operation_worker_control.clone();
                                                let progress_hook: std::sync::Arc<dyn Fn(flowrt::OperationId, u64) + Send + Sync> = std::sync::Arc::new(move |progress_id, sequence| {
                                                    operation_progress_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_progress(progress_id, sequence);
                                                });
                                                let mut progress = flowrt::OperationProgressPublisher::<PlanFeedback>::with_hook(id, progress_hook);
                                                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                                    operation_worker_server
.lock()
.unwrap_or_else(|poisoned| poisoned.into_inner())
.on_plan_operation(&goal_for_worker, cancel.clone(), &mut progress)
                                                }));
                                                let terminal_state = match result {
                                                    Ok(flowrt::OperationHandlerResult::Succeeded(_)) => flowrt::OperationState::Succeeded,
                                                    Ok(flowrt::OperationHandlerResult::Failed) | Err(_) => flowrt::OperationState::Failed,
                                                    Ok(flowrt::OperationHandlerResult::Canceled) => flowrt::OperationState::Cancelled,
                                                };
                                                let _ = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, terminal_state);
                                            });
                                        if spawn_result.is_err() {
                                            let _ = operation_start_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, flowrt::OperationState::Failed);
                                            return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError);
                                        }
                                        flowrt::ServiceResult::ok(ack)
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                if let Some(cancel_server) = __flowrt_operation_cancel_server_navigator_plan.get() {
                                    let mut cancel_server = cancel_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_cancel_control_0 = __flowrt_operation_control_0.clone();
                                    if cancel_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
                                        let mut control = operation_cancel_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                        let snapshot = control.snapshot();
                                        match control.request_cancel(id, snapshot.owner) {
                                            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),
                                            Err(error) => flowrt_operation_control_error(error),
                                        }
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                if let Some(status_server) = __flowrt_operation_status_server_navigator_plan.get() {
                                    let mut status_server = status_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_status_control_0 = __flowrt_operation_control_0.clone();
                                    if status_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
                                        match operation_status_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status(id) {
                                            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),
                                            Err(error) => flowrt_operation_control_error(error),
                                        }
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                let mut operation_control = __flowrt_operation_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let _ = operation_control.check_deadline(flowrt::monotonic_time_ms());
                                let snapshot = operation_control.snapshot();
                                let events = operation_control.drain_events();
                                drop(operation_control);
                                for event in events {
                                    let operation_id = flowrt_operation_id_string(event.id);
                                    match event.kind {
                                        flowrt::OperationRuntimeEventKind::StateChanged => {
                                            if let Some(state) = event.state {
                                                introspection_state.record_operation_transition("controller.plan", &operation_id, state.as_str(), Some("controller.plan"), if state.is_terminal() { None } else { Some(snapshot.deadline_ms) });
                                            }
                                        }
                                        flowrt::OperationRuntimeEventKind::Progress => {
                                            introspection_state.record_operation_progress("controller.plan", &operation_id, event.sequence.unwrap_or(0));
                                        }
                                        flowrt::OperationRuntimeEventKind::Result => {
                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or("succeeded");
                                            introspection_state.record_operation_result("controller.plan", &operation_id, result, None);
                                        }
                                        flowrt::OperationRuntimeEventKind::Error => {
                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or("failed");
                                            introspection_state.record_operation_result("controller.plan", &operation_id, result, Some("handler error"));
                                        }
                                    }
                                }
                                introspection_state.record_operation_health(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", snapshot));
                                flowrt::TaskRunOutcome::new(flowrt::Status::Ok, Vec::new())
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            for (name, health) in local_health_map {
                                health_map.insert(name, health);
                            }
                            pending_task_results.insert(admission.task, flowrt::TaskRunOutput::from_outcome(admission.task, task_outcome));
                            Ok(())
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
                                    let health = health_map.entry("controller.main".to_string()).or_default();
                                    health.name = "controller.main".to_string();
                                    health.lane = "controller_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                flowrt::TaskId(2) => {
                                    let health = health_map.entry("navigator.main".to_string()).or_default();
                                    health.name = "navigator.main".to_string();
                                    health.lane = "navigator_serial".to_string();
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
                            let health = health_map.entry("controller.main".to_string()).or_default();
                            health.name = "controller.main".to_string();
                            health.lane = "controller_serial".to_string();
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
                        flowrt::TaskId(2) => {
                            let health = health_map.entry("navigator.main".to_string()).or_default();
                            health.name = "navigator.main".to_string();
                            health.lane = "navigator_serial".to_string();
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
                    if health.lane == "controller_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(3)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "navigator_operation_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(2)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "navigator_serial" {
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
                let next_periodic_deadline_ms = [scheduler.next_deadline_ms(flowrt::TaskId(1)), scheduler.next_deadline_ms(flowrt::TaskId(2))].into_iter().flatten().min();
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
        if navigator_started {
            let stop_status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("navigator", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if controller_started {
            let stop_status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("controller", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if navigator_initialized {
            let shutdown_status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("navigator", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if controller_initialized {
            let shutdown_status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("controller", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if let Ok(__flowrt_status_out) = std::env::var("FLOWRT_STATUS_OUT") {
            match serde_json::to_string_pretty(&introspection_state.status()) {
                Ok(__flowrt_status_json) => {
                    if let Err(error) = std::fs::write(&__flowrt_status_out, format!("{__flowrt_status_json}\n")) {
                        eprintln!("FlowRT: failed to write FLOWRT_STATUS_OUT `{}`: {error}", __flowrt_status_out);
                    }
                }
                Err(error) => {
                    eprintln!("FlowRT: failed to encode FLOWRT_STATUS_OUT status: {error}");
                }
            }
        }
        status
    }
    pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str, run_ticks: Option<usize>) -> flowrt::Status {
        match process {
            "client_proc" => self.run_process_client_proc(backend, run_ticks),
            "server_proc" => self.run_process_server_proc(backend, run_ticks),
            _ => flowrt::Status::Error,
        }
    }
    fn run_process_client_proc(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {
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
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let _introspection_server = flowrt::spawn_status_server(
            flowrt::IntrospectionIdentity {
                self_description_hash: selfdesc::self_description_hash().to_string(),
                package: PACKAGE_NAME.to_string(),
                process: "client_proc".to_string(),
                runtime: "rust".to_string(),
            },
            introspection_state.clone(),
        )
        .ok();
        let mut controller_initialized = false;
        let mut controller_started = false;
        introspection_state.record_lifecycle_state("controller", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            controller_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("controller", if controller_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && controller_initialized {
            status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            controller_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("controller", if controller_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_client_proc_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if status == flowrt::Status::Ok {
        let _ = app.operation_client_controller_plan.start_client.set(match flowrt::iox2::Iox2FrameServiceClient::<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start") {
            Ok(client) => client,
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation start client {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_start");
                status = flowrt::Status::Error;
                flowrt::iox2::Iox2FrameServiceClient::<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::unavailable("FlowRT/service/__flowrt_operation_controller_plan_start", error.to_string())
            }
        });
        let _ = app.operation_client_controller_plan.cancel_client.set(match flowrt::iox2::Iox2ServiceClient::open("FlowRT/service/__flowrt_operation_controller_plan_cancel") {
            Ok(client) => client,
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation cancel client {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_cancel");
                status = flowrt::Status::Error;
                flowrt::iox2::Iox2ServiceClient::unavailable("FlowRT/service/__flowrt_operation_controller_plan_cancel", error.to_string())
            }
        });
        let _ = app.operation_client_controller_plan.status_client.set(match flowrt::iox2::Iox2ServiceClient::open("FlowRT/service/__flowrt_operation_controller_plan_status") {
            Ok(client) => client,
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation status client {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_status");
                status = flowrt::Status::Error;
                flowrt::iox2::Iox2ServiceClient::unavailable("FlowRT/service/__flowrt_operation_controller_plan_status", error.to_string())
            }
        });
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "controller_serial";
        scheduler.add_lane(flowrt::LaneId(2), flowrt::LaneKind::Serial);
        let _ = "navigator_operation_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(1), period_ms: 100 });
        scheduler.wake(flowrt::TaskId(1));
        // Operation task 2: controller.plan
scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 });
        let scheduler_base_period_ms: u64 = 100;
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
                let __h = health_map.entry("controller.main".to_string()).or_default();
                __h.name = "controller.main".to_string();
                __h.lane = "controller_serial".to_string();
            }
            let mut flowrt_operation_tick_driven_0 = false;
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                    let flowrt_operation_snapshot_0 = app.operation_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot();
    let flowrt_operation_active_0 = !flowrt_operation_snapshot_0.state.is_terminal()
    && flowrt_operation_snapshot_0.state != flowrt::OperationState::Idle;
    if (app.operation_start_server_navigator_plan.get().is_some()
                         || app.operation_cancel_server_navigator_plan.get().is_some()
                         || app.operation_status_server_navigator_plan.get().is_some()) && !flowrt_operation_tick_driven_0
                         || flowrt_operation_active_0 && !flowrt_operation_tick_driven_0 {
    scheduler.wake(flowrt::TaskId(2));
    flowrt_operation_tick_driven_0 = true;
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
                            let __flowrt_component_controller = app.controller.clone();
                            let __flowrt_operation_client_controller_plan = app.operation_client_controller_plan.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_name = "controller.main";
                            let task_trigger = "periodic";
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
                                Self::step_process_client_proc_task_controller_main(__flowrt_component_controller, __flowrt_operation_client_controller_plan, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            for (name, health) in local_health_map {
                                health_map.insert(name, health);
                            }
                            pending_task_results.insert(admission.task, flowrt::TaskRunOutput::from_outcome(admission.task, task_outcome));
                            Ok(())
                        },
                        flowrt::TaskId(2) => {
                            let __flowrt_operation_start_server_navigator_plan = app.operation_start_server_navigator_plan.clone();
                            let __flowrt_operation_cancel_server_navigator_plan = app.operation_cancel_server_navigator_plan.clone();
                            let __flowrt_operation_status_server_navigator_plan = app.operation_status_server_navigator_plan.clone();
                            let __flowrt_operation_server_0 = app.navigator.clone();
                            let __flowrt_operation_control_0 = app.operation_control_0.clone();
                            let introspection_state = introspection_state.clone();
                            let task_name = "__flowrt_operation.controller.plan";
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = 'flowrt_task: {
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(2));
                                let operation_cancel_control = __flowrt_operation_control_0.clone();
                                introspection_state.register_operation_cancel_handler("controller.plan", move |operation_id| {
                                    let mut control = operation_cancel_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let snapshot = control.snapshot();
                                    if flowrt_operation_id_string(snapshot.id) != operation_id {
                                        return Err(format!("stale operation invocation `{}`; current is `{}`", operation_id, flowrt_operation_id_string(snapshot.id)));
                                    }
                                    control.request_cancel(snapshot.id, snapshot.owner).map_err(|error| error.to_string())?;
                                    Ok(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", control.snapshot()))
                                });
                                if let Some(start_server) = __flowrt_operation_start_server_navigator_plan.get() {
                                    let mut start_server = start_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_start_control_0 = __flowrt_operation_control_0.clone();
                                    let operation_server_0 = __flowrt_operation_server_0.clone();
                                    if start_server.poll_requests(move |request: flowrt::OperationStartRequest<PlanGoal>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {
                                        let ack = match operation_start_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout()) {
                                            Ok(ack) => ack,
                                            Err(error) => return flowrt_operation_control_error(error),
                                        };
                                        let id = ack.id;
                                        let operation_worker_server = operation_server_0.clone();
                                        let operation_worker_control = operation_start_control_0.clone();
                                        let goal_for_worker = request.goal;
                                        let spawn_result = std::thread::Builder::new()
                                            .name("flowrt-operation-0".to_string())
                                            .spawn(move || {
                                                loop {
                                                    let should_start = {
                                                        let control = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        let status = match control.status(id) {
                                                            Ok(status) => status,
                                                            Err(_) => return,
                                                        };
                                                        if status.state.is_terminal() {
                                                            return;
                                                        }
                                                        control.ready_to_run(id)
                                                    };
                                                    if should_start {
                                                        break;
                                                    }
                                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                                }
                                                let cancel = match operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token_for(id) {
                                                    Some(cancel) => cancel,
                                                    None => return,
                                                };
                                                if operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id).is_err() {
                                                    return;
                                                }
                                                let operation_progress_control = operation_worker_control.clone();
                                                let progress_hook: std::sync::Arc<dyn Fn(flowrt::OperationId, u64) + Send + Sync> = std::sync::Arc::new(move |progress_id, sequence| {
                                                    operation_progress_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_progress(progress_id, sequence);
                                                });
                                                let mut progress = flowrt::OperationProgressPublisher::<PlanFeedback>::with_hook(id, progress_hook);
                                                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                                    operation_worker_server
.lock()
.unwrap_or_else(|poisoned| poisoned.into_inner())
.on_plan_operation(&goal_for_worker, cancel.clone(), &mut progress)
                                                }));
                                                let terminal_state = match result {
                                                    Ok(flowrt::OperationHandlerResult::Succeeded(_)) => flowrt::OperationState::Succeeded,
                                                    Ok(flowrt::OperationHandlerResult::Failed) | Err(_) => flowrt::OperationState::Failed,
                                                    Ok(flowrt::OperationHandlerResult::Canceled) => flowrt::OperationState::Cancelled,
                                                };
                                                let _ = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, terminal_state);
                                            });
                                        if spawn_result.is_err() {
                                            let _ = operation_start_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, flowrt::OperationState::Failed);
                                            return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError);
                                        }
                                        flowrt::ServiceResult::ok(ack)
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                if let Some(cancel_server) = __flowrt_operation_cancel_server_navigator_plan.get() {
                                    let mut cancel_server = cancel_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_cancel_control_0 = __flowrt_operation_control_0.clone();
                                    if cancel_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
                                        let mut control = operation_cancel_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                        let snapshot = control.snapshot();
                                        match control.request_cancel(id, snapshot.owner) {
                                            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),
                                            Err(error) => flowrt_operation_control_error(error),
                                        }
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                if let Some(status_server) = __flowrt_operation_status_server_navigator_plan.get() {
                                    let mut status_server = status_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_status_control_0 = __flowrt_operation_control_0.clone();
                                    if status_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
                                        match operation_status_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status(id) {
                                            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),
                                            Err(error) => flowrt_operation_control_error(error),
                                        }
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                let mut operation_control = __flowrt_operation_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let _ = operation_control.check_deadline(flowrt::monotonic_time_ms());
                                let snapshot = operation_control.snapshot();
                                let events = operation_control.drain_events();
                                drop(operation_control);
                                for event in events {
                                    let operation_id = flowrt_operation_id_string(event.id);
                                    match event.kind {
                                        flowrt::OperationRuntimeEventKind::StateChanged => {
                                            if let Some(state) = event.state {
                                                introspection_state.record_operation_transition("controller.plan", &operation_id, state.as_str(), Some("controller.plan"), if state.is_terminal() { None } else { Some(snapshot.deadline_ms) });
                                            }
                                        }
                                        flowrt::OperationRuntimeEventKind::Progress => {
                                            introspection_state.record_operation_progress("controller.plan", &operation_id, event.sequence.unwrap_or(0));
                                        }
                                        flowrt::OperationRuntimeEventKind::Result => {
                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or("succeeded");
                                            introspection_state.record_operation_result("controller.plan", &operation_id, result, None);
                                        }
                                        flowrt::OperationRuntimeEventKind::Error => {
                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or("failed");
                                            introspection_state.record_operation_result("controller.plan", &operation_id, result, Some("handler error"));
                                        }
                                    }
                                }
                                introspection_state.record_operation_health(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", snapshot));
                                flowrt::TaskRunOutcome::new(flowrt::Status::Ok, Vec::new())
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            for (name, health) in local_health_map {
                                health_map.insert(name, health);
                            }
                            pending_task_results.insert(admission.task, flowrt::TaskRunOutput::from_outcome(admission.task, task_outcome));
                            Ok(())
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
                                    let health = health_map.entry("controller.main".to_string()).or_default();
                                    health.name = "controller.main".to_string();
                                    health.lane = "controller_serial".to_string();
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
                            let health = health_map.entry("controller.main".to_string()).or_default();
                            health.name = "controller.main".to_string();
                            health.lane = "controller_serial".to_string();
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
                    if health.lane == "controller_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(2)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "navigator_operation_serial" {
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
                let next_periodic_deadline_ms = [scheduler.next_deadline_ms(flowrt::TaskId(1))].into_iter().flatten().min();
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
            status = app.step_process_client_proc_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if controller_started {
            let stop_status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("controller", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if controller_initialized {
            let shutdown_status = app.controller.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("controller", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if let Ok(__flowrt_status_out) = std::env::var("FLOWRT_STATUS_OUT") {
            match serde_json::to_string_pretty(&introspection_state.status()) {
                Ok(__flowrt_status_json) => {
                    if let Err(error) = std::fs::write(&__flowrt_status_out, format!("{__flowrt_status_json}\n")) {
                        eprintln!("FlowRT: failed to write FLOWRT_STATUS_OUT `{}`: {error}", __flowrt_status_out);
                    }
                }
                Err(error) => {
                    eprintln!("FlowRT: failed to encode FLOWRT_STATUS_OUT status: {error}");
                }
            }
        }
        status
    }
    fn run_process_server_proc(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {
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
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let _introspection_server = flowrt::spawn_status_server(
            flowrt::IntrospectionIdentity {
                self_description_hash: selfdesc::self_description_hash().to_string(),
                package: PACKAGE_NAME.to_string(),
                process: "server_proc".to_string(),
                runtime: "rust".to_string(),
            },
            introspection_state.clone(),
        )
        .ok();
        let mut navigator_initialized = false;
        let mut navigator_started = false;
        introspection_state.record_lifecycle_state("navigator", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            navigator_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("navigator", if navigator_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && navigator_initialized {
            status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            navigator_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("navigator", if navigator_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_server_proc_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if status == flowrt::Status::Ok {
        match flowrt::iox2::Iox2FrameServiceServer::<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start", 1usize) {
            Ok(mut server) => {
                server.set_schedule_waiter(scheduler_events.clone());
                let _ = app.operation_start_server_navigator_plan.set(std::sync::Mutex::new(server));
            }
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation start server {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_start");
                status = flowrt::Status::Error;
            }
        }
        match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_cancel", 1usize) {
            Ok(mut server) => {
                server.set_schedule_waiter(scheduler_events.clone());
                let _ = app.operation_cancel_server_navigator_plan.set(std::sync::Mutex::new(server));
            }
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation cancel server {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_cancel");
                status = flowrt::Status::Error;
            }
        }
        match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_status", 1usize) {
            Ok(mut server) => {
                server.set_schedule_waiter(scheduler_events.clone());
                let _ = app.operation_status_server_navigator_plan.set(std::sync::Mutex::new(server));
            }
            Err(error) => {
                eprintln!("FlowRT: failed to open iox2 operation status server {}: {error}", "FlowRT/service/__flowrt_operation_controller_plan_status");
                status = flowrt::Status::Error;
            }
        }
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "navigator_serial";
        scheduler.add_lane(flowrt::LaneId(2), flowrt::LaneKind::Serial);
        let _ = "navigator_operation_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(1), period_ms: 1000 });
        scheduler.wake(flowrt::TaskId(1));
        // Operation task 2: controller.plan
scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 });
        let scheduler_base_period_ms: u64 = 1000;
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
                let __h = health_map.entry("navigator.main".to_string()).or_default();
                __h.name = "navigator.main".to_string();
                __h.lane = "navigator_serial".to_string();
            }
            let mut flowrt_operation_tick_driven_0 = false;
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                    let flowrt_operation_snapshot_0 = app.operation_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot();
    let flowrt_operation_active_0 = !flowrt_operation_snapshot_0.state.is_terminal()
    && flowrt_operation_snapshot_0.state != flowrt::OperationState::Idle;
    if (app.operation_start_server_navigator_plan.get().is_some()
                         || app.operation_cancel_server_navigator_plan.get().is_some()
                         || app.operation_status_server_navigator_plan.get().is_some()) && !flowrt_operation_tick_driven_0
                         || flowrt_operation_active_0 && !flowrt_operation_tick_driven_0 {
    scheduler.wake(flowrt::TaskId(2));
    flowrt_operation_tick_driven_0 = true;
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
                            let __flowrt_component_navigator = app.navigator.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "navigator.main";
                            let task_trigger = "periodic";
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
                                Self::step_process_server_proc_task_navigator_main(__flowrt_component_navigator, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                        flowrt::TaskId(2) => {
                            let __flowrt_operation_start_server_navigator_plan = app.operation_start_server_navigator_plan.clone();
                            let __flowrt_operation_cancel_server_navigator_plan = app.operation_cancel_server_navigator_plan.clone();
                            let __flowrt_operation_status_server_navigator_plan = app.operation_status_server_navigator_plan.clone();
                            let __flowrt_operation_server_0 = app.navigator.clone();
                            let __flowrt_operation_control_0 = app.operation_control_0.clone();
                            let introspection_state = introspection_state.clone();
                            let task_name = "__flowrt_operation.controller.plan";
                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();
                            let task_outcome = 'flowrt_task: {
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(2));
                                let operation_cancel_control = __flowrt_operation_control_0.clone();
                                introspection_state.register_operation_cancel_handler("controller.plan", move |operation_id| {
                                    let mut control = operation_cancel_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let snapshot = control.snapshot();
                                    if flowrt_operation_id_string(snapshot.id) != operation_id {
                                        return Err(format!("stale operation invocation `{}`; current is `{}`", operation_id, flowrt_operation_id_string(snapshot.id)));
                                    }
                                    control.request_cancel(snapshot.id, snapshot.owner).map_err(|error| error.to_string())?;
                                    Ok(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", control.snapshot()))
                                });
                                if let Some(start_server) = __flowrt_operation_start_server_navigator_plan.get() {
                                    let mut start_server = start_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_start_control_0 = __flowrt_operation_control_0.clone();
                                    let operation_server_0 = __flowrt_operation_server_0.clone();
                                    if start_server.poll_requests(move |request: flowrt::OperationStartRequest<PlanGoal>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {
                                        let ack = match operation_start_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout()) {
                                            Ok(ack) => ack,
                                            Err(error) => return flowrt_operation_control_error(error),
                                        };
                                        let id = ack.id;
                                        let operation_worker_server = operation_server_0.clone();
                                        let operation_worker_control = operation_start_control_0.clone();
                                        let goal_for_worker = request.goal;
                                        let spawn_result = std::thread::Builder::new()
                                            .name("flowrt-operation-0".to_string())
                                            .spawn(move || {
                                                loop {
                                                    let should_start = {
                                                        let control = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        let status = match control.status(id) {
                                                            Ok(status) => status,
                                                            Err(_) => return,
                                                        };
                                                        if status.state.is_terminal() {
                                                            return;
                                                        }
                                                        control.ready_to_run(id)
                                                    };
                                                    if should_start {
                                                        break;
                                                    }
                                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                                }
                                                let cancel = match operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token_for(id) {
                                                    Some(cancel) => cancel,
                                                    None => return,
                                                };
                                                if operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id).is_err() {
                                                    return;
                                                }
                                                let operation_progress_control = operation_worker_control.clone();
                                                let progress_hook: std::sync::Arc<dyn Fn(flowrt::OperationId, u64) + Send + Sync> = std::sync::Arc::new(move |progress_id, sequence| {
                                                    operation_progress_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_progress(progress_id, sequence);
                                                });
                                                let mut progress = flowrt::OperationProgressPublisher::<PlanFeedback>::with_hook(id, progress_hook);
                                                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                                    operation_worker_server
.lock()
.unwrap_or_else(|poisoned| poisoned.into_inner())
.on_plan_operation(&goal_for_worker, cancel.clone(), &mut progress)
                                                }));
                                                let terminal_state = match result {
                                                    Ok(flowrt::OperationHandlerResult::Succeeded(_)) => flowrt::OperationState::Succeeded,
                                                    Ok(flowrt::OperationHandlerResult::Failed) | Err(_) => flowrt::OperationState::Failed,
                                                    Ok(flowrt::OperationHandlerResult::Canceled) => flowrt::OperationState::Cancelled,
                                                };
                                                let _ = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, terminal_state);
                                            });
                                        if spawn_result.is_err() {
                                            let _ = operation_start_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, flowrt::OperationState::Failed);
                                            return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError);
                                        }
                                        flowrt::ServiceResult::ok(ack)
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                if let Some(cancel_server) = __flowrt_operation_cancel_server_navigator_plan.get() {
                                    let mut cancel_server = cancel_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_cancel_control_0 = __flowrt_operation_control_0.clone();
                                    if cancel_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
                                        let mut control = operation_cancel_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                        let snapshot = control.snapshot();
                                        match control.request_cancel(id, snapshot.owner) {
                                            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),
                                            Err(error) => flowrt_operation_control_error(error),
                                        }
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                if let Some(status_server) = __flowrt_operation_status_server_navigator_plan.get() {
                                    let mut status_server = status_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    let operation_status_control_0 = __flowrt_operation_control_0.clone();
                                    if status_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
                                        match operation_status_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status(id) {
                                            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),
                                            Err(error) => flowrt_operation_control_error(error),
                                        }
                                    }).is_err() {
                                        break 'flowrt_task flowrt::TaskRunOutcome::new(flowrt::Status::Error, Vec::new());
                                    }
                                }
                                let mut operation_control = __flowrt_operation_control_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let _ = operation_control.check_deadline(flowrt::monotonic_time_ms());
                                let snapshot = operation_control.snapshot();
                                let events = operation_control.drain_events();
                                drop(operation_control);
                                for event in events {
                                    let operation_id = flowrt_operation_id_string(event.id);
                                    match event.kind {
                                        flowrt::OperationRuntimeEventKind::StateChanged => {
                                            if let Some(state) = event.state {
                                                introspection_state.record_operation_transition("controller.plan", &operation_id, state.as_str(), Some("controller.plan"), if state.is_terminal() { None } else { Some(snapshot.deadline_ms) });
                                            }
                                        }
                                        flowrt::OperationRuntimeEventKind::Progress => {
                                            introspection_state.record_operation_progress("controller.plan", &operation_id, event.sequence.unwrap_or(0));
                                        }
                                        flowrt::OperationRuntimeEventKind::Result => {
                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or("succeeded");
                                            introspection_state.record_operation_result("controller.plan", &operation_id, result, None);
                                        }
                                        flowrt::OperationRuntimeEventKind::Error => {
                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or("failed");
                                            introspection_state.record_operation_result("controller.plan", &operation_id, result, Some("handler error"));
                                        }
                                    }
                                }
                                introspection_state.record_operation_health(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", snapshot));
                                flowrt::TaskRunOutcome::new(flowrt::Status::Ok, Vec::new())
                            };
                            if let Some(health) = local_health_map.get_mut(task_name) {
                                health.inflight = false;
                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                health.observed_time_ms = Some(admission.observed_time_ms);
                                health.lateness_ms = Some(admission.lateness_ms);
                                health.missed_periods = Some(admission.missed_periods);
                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                            }
                            for (name, health) in local_health_map {
                                health_map.insert(name, health);
                            }
                            pending_task_results.insert(admission.task, flowrt::TaskRunOutput::from_outcome(admission.task, task_outcome));
                            Ok(())
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
                                    let health = health_map.entry("navigator.main".to_string()).or_default();
                                    health.name = "navigator.main".to_string();
                                    health.lane = "navigator_serial".to_string();
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
                            let health = health_map.entry("navigator.main".to_string()).or_default();
                            health.name = "navigator.main".to_string();
                            health.lane = "navigator_serial".to_string();
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
            if scheduler.lane_starvation_ticks(flowrt::LaneId(2)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "navigator_operation_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(1)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "navigator_serial" {
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
                let next_periodic_deadline_ms = [scheduler.next_deadline_ms(flowrt::TaskId(1))].into_iter().flatten().min();
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
            status = app.step_process_server_proc_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if navigator_started {
            let stop_status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("navigator", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if navigator_initialized {
            let shutdown_status = app.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("navigator", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if let Ok(__flowrt_status_out) = std::env::var("FLOWRT_STATUS_OUT") {
            match serde_json::to_string_pretty(&introspection_state.status()) {
                Ok(__flowrt_status_json) => {
                    if let Err(error) = std::fs::write(&__flowrt_status_out, format!("{__flowrt_status_json}\n")) {
                        eprintln!("FlowRT: failed to write FLOWRT_STATUS_OUT `{}`: {error}", __flowrt_status_out);
                    }
                }
                Err(error) => {
                    eprintln!("FlowRT: failed to encode FLOWRT_STATUS_OUT status: {error}");
                }
            }
        }
        status
    }
}

pub fn backend() -> Box<dyn flowrt::Backend> {
    Box::new(flowrt::iox2_backend())
}

pub fn run(run_ticks: Option<usize>) -> flowrt::Status {
    let backend = backend();
    user::build_app().run(backend.as_ref(), run_ticks)
}

pub fn run_process(process: &str, run_ticks: Option<usize>) -> flowrt::Status {
    let backend = backend();
    user::build_app().run_process(backend.as_ref(), process, run_ticks)
}
