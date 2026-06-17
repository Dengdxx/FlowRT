// FlowRT 管理产物。不要手工修改。

use crate::components::*;
use crate::messages::*;
use crate::selfdesc;
use crate::user;

const PACKAGE_NAME: &str = "graph_demo";

type FlowrtOutputCommit = Box<dyn FnOnce(&App, &flowrt::IntrospectionState, &flowrt::ScheduleWaiter, &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status + Send>;

fn register_introspection_channel(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    max_payload_len: Option<usize>,
) -> flowrt::IntrospectionChannelProbe {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.register_channel_with_probe_capacity(name, message_type, max_payload_len);
        state.channel_probe(name).unwrap_or_default()
    }))
    .unwrap_or_default()
}

#[allow(dead_code)]
fn record_introspection_input_read<T>(
    state: &flowrt::IntrospectionState,
    key: &'static str,
    task: &'static str,
    input: &'static str,
    channel: &'static str,
    message_type: &'static str,
    value: &flowrt::Latest<'_, T>,
    revision: u64,
    tick_time_ms: u64,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.record_input_read(
            key,
            task,
            input,
            channel,
            message_type,
            value.present(),
            value.stale(),
            Some(revision),
            Some(tick_time_ms),
        );
    }));
}

#[allow(dead_code)]
fn record_introspection_publish_copy<T: Copy>(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    probe: &flowrt::IntrospectionChannelProbe,
    value: &T,
    published_at_ms: u64,
) {
    probe.record_publish_event();
    if !probe.enabled() && !state.recorder_enabled_for_channel(name) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let payload = unsafe {
            std::slice::from_raw_parts(
                (value as *const T).cast::<u8>(),
                std::mem::size_of::<T>(),
            )
        };
        state.try_record_channel_sample_bytes(name, message_type, payload, Some(published_at_ms));
        if probe.enabled() {
            probe.try_record_bytes(payload, Some(published_at_ms));
        }
    }));
}

#[allow(dead_code)]
fn record_introspection_publish_frame<T: flowrt::FrameCodec>(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    probe: &flowrt::IntrospectionChannelProbe,
    value: &T,
    published_at_ms: u64,
) {
    probe.record_publish_event();
    if !probe.enabled() && !state.recorder_enabled_for_channel(name) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(payload) = value.to_frame_vec() {
            state.try_record_channel_sample_frame_bytes(
                name,
                message_type,
                &payload,
                Some(published_at_ms),
            );
            if probe.enabled() {
                probe.try_record_bytes(&payload, Some(published_at_ms));
            }
        }
    }));
}

pub struct App {
    startup_status: flowrt::Status,
    imu_sim: std::sync::Arc<std::sync::Mutex<Box<dyn ImuSim + Send>>>,
    estimator: std::sync::Arc<std::sync::Mutex<Box<dyn Estimator + Send>>>,
    monitor: std::sync::Arc<std::sync::Mutex<Box<dyn Monitor + Send>>>,
    bind_0: std::sync::Arc<std::sync::Mutex<flowrt::FifoChannel<Odom>>>,
    introspection_probe_bind_0: std::sync::OnceLock<flowrt::IntrospectionChannelProbe>,
    bind_1: std::sync::Arc<std::sync::Mutex<flowrt::LatestChannel<Imu>>>,
    introspection_probe_bind_1: std::sync::OnceLock<flowrt::IntrospectionChannelProbe>,
    bind_2: std::sync::Arc<std::sync::Mutex<flowrt::FifoChannel<Imu>>>,
    introspection_probe_bind_2: std::sync::OnceLock<flowrt::IntrospectionChannelProbe>,
}

impl App {
    pub fn new(
        imu_sim: Box<dyn ImuSim + Send>,
        estimator: Box<dyn Estimator + Send>,
        monitor: Box<dyn Monitor + Send>,
    ) -> Self {
        let startup_status = flowrt::Status::Ok;
        let imu_sim = std::sync::Arc::new(std::sync::Mutex::new(imu_sim));
        let estimator = std::sync::Arc::new(std::sync::Mutex::new(estimator));
        let monitor = std::sync::Arc::new(std::sync::Mutex::new(monitor));
        Self {
            imu_sim: imu_sim.clone(),
            estimator: estimator.clone(),
            monitor: monitor.clone(),
            bind_0: std::sync::Arc::new(std::sync::Mutex::new(flowrt::FifoChannel::with_stale_config(8, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Warn)))),
            introspection_probe_bind_0: std::sync::OnceLock::new(),
            bind_1: std::sync::Arc::new(std::sync::Mutex::new(flowrt::LatestChannel::with_stale_config(flowrt::StaleConfig::new(None, flowrt::StalePolicy::Warn)))),
            introspection_probe_bind_1: std::sync::OnceLock::new(),
            bind_2: std::sync::Arc::new(std::sync::Mutex::new(flowrt::FifoChannel::new(8, flowrt::OverflowPolicy::DropNewest))),
            introspection_probe_bind_2: std::sync::OnceLock::new(),
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
            {
                let __h = health_map.entry("imu_sim.main".to_string()).or_default();
                __h.name = "imu_sim.main".to_string();
                __h.lane = "imu_sim_serial".to_string();
            }
            let mut imu = flowrt::Output::<Imu>::new();
            match self.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut imu) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
            if let Some(value) = imu.as_ref().cloned() {
                self.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms);
                introspection_state.record_route_publish("imu_sim.imu_to_estimator.imu", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_1_probe) = self.introspection_probe_bind_1.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_estimator.imu", "Imu", introspection_probe_bind_1_probe, &value, tick_time_ms);
                }
                match self.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                    Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = self.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = self.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                    }
                    Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                        introspection_state.record_route_backpressure("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().backpressure += 1;
                        return flowrt::Status::Retry;
                    }
                    Err(flowrt::ChannelError::Overflow) => {
                        introspection_state.record_route_overflow("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().overflow += 1;
                        return flowrt::Status::Error;
                    }
                }
            }
        }
        {
            let __flowrt_bind_1_guard = self.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let imu = __flowrt_bind_1_guard.view_at(tick_time_ms);
            let __flowrt_imu_revision = __flowrt_bind_1_guard.revision();
            record_introspection_input_read(&introspection_state, "estimator.main.imu", "estimator.main", "imu", "imu_sim.imu_to_estimator.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("estimator.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("estimator.main".to_string()).or_default();
                __h.name = "estimator.main".to_string();
                __h.lane = "estimator_serial".to_string();
            }
            if imu.present() {
                let mut odom = flowrt::Output::<Odom>::new();
                match self.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, &mut odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
                }
                if let Some(value) = odom.as_ref().cloned() {
                    match self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                        Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = self.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = self.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                        }
                        Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                            introspection_state.record_route_backpressure("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().backpressure += 1;
                            return flowrt::Status::Retry;
                        }
                        Err(flowrt::ChannelError::Overflow) => {
                            introspection_state.record_route_overflow("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().overflow += 1;
                            return flowrt::Status::Error;
                        }
                    }
                }
            }
        }
        {
            let mut __flowrt_bind_2_guard = self.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let imu_read = __flowrt_bind_2_guard.pop_at(tick_time_ms);
            let __flowrt_imu_revision = __flowrt_bind_2_guard.revision();
            let imu = imu_read.view();
            record_introspection_input_read(&introspection_state, "monitor.main.imu", "monitor.main", "imu", "imu_sim.imu_to_monitor.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            let mut __flowrt_bind_0_guard = self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let odom_read = __flowrt_bind_0_guard.pop_at(tick_time_ms);
            let __flowrt_odom_revision = __flowrt_bind_0_guard.revision();
            let odom = odom_read.view();
            record_introspection_input_read(&introspection_state, "monitor.main.odom", "monitor.main", "odom", "estimator.odom_to_monitor.odom", "Odom", &odom, __flowrt_odom_revision, tick_time_ms);
            if odom.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("monitor.main".to_string()).or_default();
                __h.name = "monitor.main".to_string();
                __h.lane = "monitor_serial".to_string();
            }
            if imu.present() || odom.present() {
                match self.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
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
    fn step_task_estimator_main(
        __flowrt_component_estimator: std::sync::Arc<std::sync::Mutex<Box<dyn Estimator + Send>>>,
        __flowrt_input_imu_value: Option<Imu>,
        __flowrt_input_imu_stale: bool,
        __flowrt_input_imu_revision: u64,
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
            let imu = flowrt::Latest::new(__flowrt_input_imu_value.as_ref(), __flowrt_input_imu_stale);
            let __flowrt_imu_revision = __flowrt_input_imu_revision;
            record_introspection_input_read(&introspection_state, "estimator.main.imu", "estimator.main", "imu", "imu_sim.imu_to_estimator.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("estimator.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("estimator.main".to_string()).or_default();
                __h.name = "estimator.main".to_string();
                __h.lane = "estimator_serial".to_string();
            }
            if imu.present() {
                let mut odom = flowrt::Output::<Odom>::new();
                match __flowrt_component_estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, &mut odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
                }
                if let Some(value) = odom.as_ref().cloned() {
                    let value = value.clone();
                    __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, health_map| {
                    match app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                        Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = app.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = app.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                        }
                        Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                            introspection_state.record_route_backpressure("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().backpressure += 1;
                            return flowrt::Status::Retry;
                        }
                        Err(flowrt::ChannelError::Overflow) => {
                            introspection_state.record_route_overflow("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().overflow += 1;
                            return flowrt::Status::Error;
                        }
                    }
                        flowrt::Status::Ok
                    }));
                }
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_task_imu_sim_main(
        __flowrt_component_imu_sim: std::sync::Arc<std::sync::Mutex<Box<dyn ImuSim + Send>>>,
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
            {
                let __h = health_map.entry("imu_sim.main".to_string()).or_default();
                __h.name = "imu_sim.main".to_string();
                __h.lane = "imu_sim_serial".to_string();
            }
            let mut imu = flowrt::Output::<Imu>::new();
            match __flowrt_component_imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut imu) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
            if let Some(value) = imu.as_ref().cloned() {
                let value = value.clone();
                __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, _health_map| {
                app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms);
                introspection_state.record_route_publish("imu_sim.imu_to_estimator.imu", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_1_probe) = app.introspection_probe_bind_1.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_estimator.imu", "Imu", introspection_probe_bind_1_probe, &value, tick_time_ms);
                }
                    flowrt::Status::Ok
                }));
                let value = value.clone();
                __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, health_map| {
                match app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                    Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = app.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = app.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                    }
                    Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                        introspection_state.record_route_backpressure("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().backpressure += 1;
                        return flowrt::Status::Retry;
                    }
                    Err(flowrt::ChannelError::Overflow) => {
                        introspection_state.record_route_overflow("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().overflow += 1;
                        return flowrt::Status::Error;
                    }
                }
                    flowrt::Status::Ok
                }));
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_task_monitor_main(
        __flowrt_component_monitor: std::sync::Arc<std::sync::Mutex<Box<dyn Monitor + Send>>>,
        __flowrt_input_imu_value: Option<Imu>,
        __flowrt_input_imu_stale: bool,
        __flowrt_input_imu_revision: u64,
        __flowrt_input_odom_value: Option<Odom>,
        __flowrt_input_odom_stale: bool,
        __flowrt_input_odom_revision: u64,
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
            let imu = flowrt::Latest::new(__flowrt_input_imu_value.as_ref(), __flowrt_input_imu_stale);
            let __flowrt_imu_revision = __flowrt_input_imu_revision;
            record_introspection_input_read(&introspection_state, "monitor.main.imu", "monitor.main", "imu", "imu_sim.imu_to_monitor.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            let odom = flowrt::Latest::new(__flowrt_input_odom_value.as_ref(), __flowrt_input_odom_stale);
            let __flowrt_odom_revision = __flowrt_input_odom_revision;
            record_introspection_input_read(&introspection_state, "monitor.main.odom", "monitor.main", "odom", "estimator.odom_to_monitor.odom", "Odom", &odom, __flowrt_odom_revision, tick_time_ms);
            if odom.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("monitor.main".to_string()).or_default();
                __h.name = "monitor.main".to_string();
                __h.lane = "monitor_serial".to_string();
            }
            if imu.present() || odom.present() {
                match __flowrt_component_monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
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
            {
                let __h = health_map.entry("imu_sim.main".to_string()).or_default();
                __h.name = "imu_sim.main".to_string();
                __h.lane = "imu_sim_serial".to_string();
            }
            let mut imu = flowrt::Output::<Imu>::new();
            match self.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut imu) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
            if let Some(value) = imu.as_ref().cloned() {
                self.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms);
                introspection_state.record_route_publish("imu_sim.imu_to_estimator.imu", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_1_probe) = self.introspection_probe_bind_1.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_estimator.imu", "Imu", introspection_probe_bind_1_probe, &value, tick_time_ms);
                }
                match self.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                    Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = self.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = self.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                    }
                    Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                        introspection_state.record_route_backpressure("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().backpressure += 1;
                        return flowrt::Status::Retry;
                    }
                    Err(flowrt::ChannelError::Overflow) => {
                        introspection_state.record_route_overflow("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().overflow += 1;
                        return flowrt::Status::Error;
                    }
                }
            }
        }
        {
            let __flowrt_bind_1_guard = self.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let imu = __flowrt_bind_1_guard.view_at(tick_time_ms);
            let __flowrt_imu_revision = __flowrt_bind_1_guard.revision();
            record_introspection_input_read(&introspection_state, "estimator.main.imu", "estimator.main", "imu", "imu_sim.imu_to_estimator.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("estimator.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("estimator.main".to_string()).or_default();
                __h.name = "estimator.main".to_string();
                __h.lane = "estimator_serial".to_string();
            }
            if imu.present() {
                let mut odom = flowrt::Output::<Odom>::new();
                match self.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, &mut odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
                }
                if let Some(value) = odom.as_ref().cloned() {
                    match self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                        Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = self.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = self.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                        }
                        Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                            introspection_state.record_route_backpressure("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().backpressure += 1;
                            return flowrt::Status::Retry;
                        }
                        Err(flowrt::ChannelError::Overflow) => {
                            introspection_state.record_route_overflow("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().overflow += 1;
                            return flowrt::Status::Error;
                        }
                    }
                }
            }
        }
        {
            let mut __flowrt_bind_2_guard = self.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let imu_read = __flowrt_bind_2_guard.pop_at(tick_time_ms);
            let __flowrt_imu_revision = __flowrt_bind_2_guard.revision();
            let imu = imu_read.view();
            record_introspection_input_read(&introspection_state, "monitor.main.imu", "monitor.main", "imu", "imu_sim.imu_to_monitor.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            let mut __flowrt_bind_0_guard = self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let odom_read = __flowrt_bind_0_guard.pop_at(tick_time_ms);
            let __flowrt_odom_revision = __flowrt_bind_0_guard.revision();
            let odom = odom_read.view();
            record_introspection_input_read(&introspection_state, "monitor.main.odom", "monitor.main", "odom", "estimator.odom_to_monitor.odom", "Odom", &odom, __flowrt_odom_revision, tick_time_ms);
            if odom.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("monitor.main".to_string()).or_default();
                __h.name = "monitor.main".to_string();
                __h.lane = "monitor_serial".to_string();
            }
            if imu.present() || odom.present() {
                match self.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
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
    fn step_process_main_task_estimator_main(
        __flowrt_component_estimator: std::sync::Arc<std::sync::Mutex<Box<dyn Estimator + Send>>>,
        __flowrt_input_imu_value: Option<Imu>,
        __flowrt_input_imu_stale: bool,
        __flowrt_input_imu_revision: u64,
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
            let imu = flowrt::Latest::new(__flowrt_input_imu_value.as_ref(), __flowrt_input_imu_stale);
            let __flowrt_imu_revision = __flowrt_input_imu_revision;
            record_introspection_input_read(&introspection_state, "estimator.main.imu", "estimator.main", "imu", "imu_sim.imu_to_estimator.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("estimator.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("estimator.main".to_string()).or_default();
                __h.name = "estimator.main".to_string();
                __h.lane = "estimator_serial".to_string();
            }
            if imu.present() {
                let mut odom = flowrt::Output::<Odom>::new();
                match __flowrt_component_estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, &mut odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
                }
                if let Some(value) = odom.as_ref().cloned() {
                    let value = value.clone();
                    __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, health_map| {
                    match app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                        Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = app.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                            introspection_state.record_route_publish("estimator.odom_to_monitor.odom", Some(tick_time_ms));
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                            scheduler_events.notify_data();
                    if let Some(introspection_probe_bind_0_probe) = app.introspection_probe_bind_0.get() {
                        record_introspection_publish_copy(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", introspection_probe_bind_0_probe, &value, tick_time_ms);
                    }
                        }
                        Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                            introspection_state.record_route_drop("estimator.odom_to_monitor.odom");
                        }
                        Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                            introspection_state.record_route_backpressure("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().backpressure += 1;
                            return flowrt::Status::Retry;
                        }
                        Err(flowrt::ChannelError::Overflow) => {
                            introspection_state.record_route_overflow("estimator.odom_to_monitor.odom");
                            health_map.entry("estimator.main".to_string()).or_default().overflow += 1;
                            return flowrt::Status::Error;
                        }
                    }
                        flowrt::Status::Ok
                    }));
                }
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_main_task_imu_sim_main(
        __flowrt_component_imu_sim: std::sync::Arc<std::sync::Mutex<Box<dyn ImuSim + Send>>>,
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
            {
                let __h = health_map.entry("imu_sim.main".to_string()).or_default();
                __h.name = "imu_sim.main".to_string();
                __h.lane = "imu_sim_serial".to_string();
            }
            let mut imu = flowrt::Output::<Imu>::new();
            match __flowrt_component_imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut imu) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
            if let Some(value) = imu.as_ref().cloned() {
                let value = value.clone();
                __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, _health_map| {
                app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms);
                introspection_state.record_route_publish("imu_sim.imu_to_estimator.imu", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_1_probe) = app.introspection_probe_bind_1.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_estimator.imu", "Imu", introspection_probe_bind_1_probe, &value, tick_time_ms);
                }
                    flowrt::Status::Ok
                }));
                let value = value.clone();
                __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, health_map| {
                match app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {
                    Ok(flowrt::ChannelWriteOutcome::Accepted) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = app.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {
                        introspection_state.record_route_publish("imu_sim.imu_to_monitor.imu", Some(tick_time_ms));
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                        scheduler_events.notify_data();
                if let Some(introspection_probe_bind_2_probe) = app.introspection_probe_bind_2.get() {
                    record_introspection_publish_copy(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", introspection_probe_bind_2_probe, &value, tick_time_ms);
                }
                    }
                    Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {
                        introspection_state.record_route_drop("imu_sim.imu_to_monitor.imu");
                    }
                    Ok(flowrt::ChannelWriteOutcome::Backpressured) => {
                        introspection_state.record_route_backpressure("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().backpressure += 1;
                        return flowrt::Status::Retry;
                    }
                    Err(flowrt::ChannelError::Overflow) => {
                        introspection_state.record_route_overflow("imu_sim.imu_to_monitor.imu");
                        health_map.entry("imu_sim.main".to_string()).or_default().overflow += 1;
                        return flowrt::Status::Error;
                    }
                }
                    flowrt::Status::Ok
                }));
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_main_task_monitor_main(
        __flowrt_component_monitor: std::sync::Arc<std::sync::Mutex<Box<dyn Monitor + Send>>>,
        __flowrt_input_imu_value: Option<Imu>,
        __flowrt_input_imu_stale: bool,
        __flowrt_input_imu_revision: u64,
        __flowrt_input_odom_value: Option<Odom>,
        __flowrt_input_odom_stale: bool,
        __flowrt_input_odom_revision: u64,
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
            let imu = flowrt::Latest::new(__flowrt_input_imu_value.as_ref(), __flowrt_input_imu_stale);
            let __flowrt_imu_revision = __flowrt_input_imu_revision;
            record_introspection_input_read(&introspection_state, "monitor.main.imu", "monitor.main", "imu", "imu_sim.imu_to_monitor.imu", "Imu", &imu, __flowrt_imu_revision, tick_time_ms);
            if imu.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            let odom = flowrt::Latest::new(__flowrt_input_odom_value.as_ref(), __flowrt_input_odom_stale);
            let __flowrt_odom_revision = __flowrt_input_odom_revision;
            record_introspection_input_read(&introspection_state, "monitor.main.odom", "monitor.main", "odom", "estimator.odom_to_monitor.odom", "Odom", &odom, __flowrt_odom_revision, tick_time_ms);
            if odom.stale() {
                health_map.entry("monitor.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("monitor.main".to_string()).or_default();
                __h.name = "monitor.main".to_string();
                __h.lane = "monitor_serial".to_string();
            }
            if imu.present() || odom.present() {
                match __flowrt_component_monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(imu, odom) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
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
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let introspection_probe_bind_0 = register_introspection_channel(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", Some(4));
        let _ = app.introspection_probe_bind_0.set(introspection_probe_bind_0);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "estimator.odom_to_monitor.odom".to_string(),
            from: "estimator.odom".to_string(),
            to: "monitor.odom".to_string(),
            message_type: "Odom".to_string(),
            backend: "inproc".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "monitor.main".to_string(),
            input: "odom".to_string(),
            channel: "estimator.odom_to_monitor.odom".to_string(),
            message_type: "Odom".to_string(),
            ..Default::default()
        });
        let introspection_probe_bind_1 = register_introspection_channel(&introspection_state, "imu_sim.imu_to_estimator.imu", "Imu", Some(4));
        let _ = app.introspection_probe_bind_1.set(introspection_probe_bind_1);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "imu_sim.imu_to_estimator.imu".to_string(),
            from: "imu_sim.imu".to_string(),
            to: "estimator.imu".to_string(),
            message_type: "Imu".to_string(),
            backend: "inproc".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "estimator.main".to_string(),
            input: "imu".to_string(),
            channel: "imu_sim.imu_to_estimator.imu".to_string(),
            message_type: "Imu".to_string(),
            ..Default::default()
        });
        let introspection_probe_bind_2 = register_introspection_channel(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", Some(4));
        let _ = app.introspection_probe_bind_2.set(introspection_probe_bind_2);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "imu_sim.imu_to_monitor.imu".to_string(),
            from: "imu_sim.imu".to_string(),
            to: "monitor.imu".to_string(),
            message_type: "Imu".to_string(),
            backend: "inproc".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "monitor.main".to_string(),
            input: "imu".to_string(),
            channel: "imu_sim.imu_to_monitor.imu".to_string(),
            message_type: "Imu".to_string(),
            ..Default::default()
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
        let mut imu_sim_initialized = false;
        let mut imu_sim_started = false;
        introspection_state.record_lifecycle_state("imu_sim", flowrt::LifecycleState::Uninitialized);
        let mut estimator_initialized = false;
        let mut estimator_started = false;
        introspection_state.record_lifecycle_state("estimator", flowrt::LifecycleState::Uninitialized);
        let mut monitor_initialized = false;
        let mut monitor_started = false;
        introspection_state.record_lifecycle_state("monitor", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            imu_sim_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("imu_sim", if imu_sim_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            estimator_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("estimator", if estimator_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            monitor_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("monitor", if monitor_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && imu_sim_initialized {
            status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            imu_sim_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("imu_sim", if imu_sim_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && estimator_initialized {
            status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            estimator_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("estimator", if estimator_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && monitor_initialized {
            status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            monitor_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("monitor", if monitor_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "estimator_serial";
        scheduler.add_lane(flowrt::LaneId(2), flowrt::LaneKind::Serial);
        let _ = "imu_sim_serial";
        scheduler.add_lane(flowrt::LaneId(3), flowrt::LaneKind::Serial);
        let _ = "monitor_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(2), period_ms: 5 });
        scheduler.wake(flowrt::TaskId(2));
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(3), lane: flowrt::LaneId(3), priority: 0 });
        let mut bind_1_seen_revision_for_estimator_main: u64 = 0;
        let mut bind_2_seen_revision_for_monitor_main: u64 = 0;
        let mut bind_0_seen_revision_for_monitor_main: u64 = 0;
        let scheduler_base_period_ms: u64 = 5;
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
                let __h = health_map.entry("estimator.main".to_string()).or_default();
                __h.name = "estimator.main".to_string();
                __h.lane = "estimator_serial".to_string();
            }
            {
                let __h = health_map.entry("imu_sim.main".to_string()).or_default();
                __h.name = "imu_sim.main".to_string();
                __h.lane = "imu_sim_serial".to_string();
            }
            {
                let __h = health_map.entry("monitor.main".to_string()).or_default();
                __h.name = "monitor.main".to_string();
                __h.lane = "monitor_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                if app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_1_seen_revision_for_estimator_main {
                    bind_1_seen_revision_for_estimator_main = app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
                    scheduler.wake(flowrt::TaskId(1));
                    woke_on_message = true;
                }
                if (app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_2_seen_revision_for_monitor_main || !app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty()) || (app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_0_seen_revision_for_monitor_main || !app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty()) {
                    bind_2_seen_revision_for_monitor_main = app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
                    bind_0_seen_revision_for_monitor_main = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
                    scheduler.wake(flowrt::TaskId(3));
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
                            let __flowrt_component_estimator = app.estimator.clone();
                            let (__flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision) = {
                                let __flowrt_bind_1_snapshot_guard = app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_imu_snapshot_view = __flowrt_bind_1_snapshot_guard.view_at(tick_time_ms);
                                (__flowrt_imu_snapshot_view.as_ref().cloned(), __flowrt_imu_snapshot_view.stale(), __flowrt_bind_1_snapshot_guard.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "estimator.main";
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
                                Self::step_task_estimator_main(__flowrt_component_estimator, __flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                            let __flowrt_component_imu_sim = app.imu_sim.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "imu_sim.main";
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
                                Self::step_task_imu_sim_main(__flowrt_component_imu_sim, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                            let __flowrt_component_monitor = app.monitor.clone();
                            let (__flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision) = {
                                let mut __flowrt_bind_2_snapshot_guard = app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_fifo_read = __flowrt_bind_2_snapshot_guard.pop_at(tick_time_ms);
                                let __flowrt_imu_snapshot_view = __flowrt_fifo_read.view();
                                (__flowrt_imu_snapshot_view.as_ref().cloned(), __flowrt_imu_snapshot_view.stale(), __flowrt_bind_2_snapshot_guard.revision())
                            };
                            let (__flowrt_input_odom_value, __flowrt_input_odom_stale, __flowrt_input_odom_revision) = {
                                let mut __flowrt_bind_0_snapshot_guard = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_fifo_read = __flowrt_bind_0_snapshot_guard.pop_at(tick_time_ms);
                                let __flowrt_odom_snapshot_view = __flowrt_fifo_read.view();
                                (__flowrt_odom_snapshot_view.as_ref().cloned(), __flowrt_odom_snapshot_view.stale(), __flowrt_bind_0_snapshot_guard.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "monitor.main";
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
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(3));
                                Self::step_task_monitor_main(__flowrt_component_monitor, __flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision, __flowrt_input_odom_value, __flowrt_input_odom_stale, __flowrt_input_odom_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                                    let health = health_map.entry("estimator.main".to_string()).or_default();
                                    health.name = "estimator.main".to_string();
                                    health.lane = "estimator_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                flowrt::TaskId(2) => {
                                    let health = health_map.entry("imu_sim.main".to_string()).or_default();
                                    health.name = "imu_sim.main".to_string();
                                    health.lane = "imu_sim_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                flowrt::TaskId(3) => {
                                    let health = health_map.entry("monitor.main".to_string()).or_default();
                                    health.name = "monitor.main".to_string();
                                    health.lane = "monitor_serial".to_string();
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
                            let health = health_map.entry("estimator.main".to_string()).or_default();
                            health.name = "estimator.main".to_string();
                            health.lane = "estimator_serial".to_string();
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
                            let health = health_map.entry("imu_sim.main".to_string()).or_default();
                            health.name = "imu_sim.main".to_string();
                            health.lane = "imu_sim_serial".to_string();
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
                        flowrt::TaskId(3) => {
                            let health = health_map.entry("monitor.main".to_string()).or_default();
                            health.name = "monitor.main".to_string();
                            health.lane = "monitor_serial".to_string();
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
                    if health.lane == "estimator_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(2)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "imu_sim_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(3)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "monitor_serial" {
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
                let next_periodic_deadline_ms = [scheduler.next_deadline_ms(flowrt::TaskId(2))].into_iter().flatten().min();
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
        if monitor_started {
            let stop_status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("monitor", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if estimator_started {
            let stop_status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("estimator", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if imu_sim_started {
            let stop_status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("imu_sim", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if monitor_initialized {
            let shutdown_status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("monitor", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if estimator_initialized {
            let shutdown_status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("estimator", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if imu_sim_initialized {
            let shutdown_status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("imu_sim", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
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
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let introspection_probe_bind_0 = register_introspection_channel(&introspection_state, "estimator.odom_to_monitor.odom", "Odom", Some(4));
        let _ = app.introspection_probe_bind_0.set(introspection_probe_bind_0);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "estimator.odom_to_monitor.odom".to_string(),
            from: "estimator.odom".to_string(),
            to: "monitor.odom".to_string(),
            message_type: "Odom".to_string(),
            backend: "inproc".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "monitor.main".to_string(),
            input: "odom".to_string(),
            channel: "estimator.odom_to_monitor.odom".to_string(),
            message_type: "Odom".to_string(),
            ..Default::default()
        });
        let introspection_probe_bind_1 = register_introspection_channel(&introspection_state, "imu_sim.imu_to_estimator.imu", "Imu", Some(4));
        let _ = app.introspection_probe_bind_1.set(introspection_probe_bind_1);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "imu_sim.imu_to_estimator.imu".to_string(),
            from: "imu_sim.imu".to_string(),
            to: "estimator.imu".to_string(),
            message_type: "Imu".to_string(),
            backend: "inproc".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "estimator.main".to_string(),
            input: "imu".to_string(),
            channel: "imu_sim.imu_to_estimator.imu".to_string(),
            message_type: "Imu".to_string(),
            ..Default::default()
        });
        let introspection_probe_bind_2 = register_introspection_channel(&introspection_state, "imu_sim.imu_to_monitor.imu", "Imu", Some(4));
        let _ = app.introspection_probe_bind_2.set(introspection_probe_bind_2);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "imu_sim.imu_to_monitor.imu".to_string(),
            from: "imu_sim.imu".to_string(),
            to: "monitor.imu".to_string(),
            message_type: "Imu".to_string(),
            backend: "inproc".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "monitor.main".to_string(),
            input: "imu".to_string(),
            channel: "imu_sim.imu_to_monitor.imu".to_string(),
            message_type: "Imu".to_string(),
            ..Default::default()
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
        let mut imu_sim_initialized = false;
        let mut imu_sim_started = false;
        introspection_state.record_lifecycle_state("imu_sim", flowrt::LifecycleState::Uninitialized);
        let mut estimator_initialized = false;
        let mut estimator_started = false;
        introspection_state.record_lifecycle_state("estimator", flowrt::LifecycleState::Uninitialized);
        let mut monitor_initialized = false;
        let mut monitor_started = false;
        introspection_state.record_lifecycle_state("monitor", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            imu_sim_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("imu_sim", if imu_sim_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            estimator_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("estimator", if estimator_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            monitor_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("monitor", if monitor_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && imu_sim_initialized {
            status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            imu_sim_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("imu_sim", if imu_sim_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && estimator_initialized {
            status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            estimator_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("estimator", if estimator_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && monitor_initialized {
            status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            monitor_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("monitor", if monitor_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_main_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "estimator_serial";
        scheduler.add_lane(flowrt::LaneId(2), flowrt::LaneKind::Serial);
        let _ = "imu_sim_serial";
        scheduler.add_lane(flowrt::LaneId(3), flowrt::LaneKind::Serial);
        let _ = "monitor_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(2), period_ms: 5 });
        scheduler.wake(flowrt::TaskId(2));
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(3), lane: flowrt::LaneId(3), priority: 0 });
        let mut bind_1_seen_revision_for_estimator_main: u64 = 0;
        let mut bind_2_seen_revision_for_monitor_main: u64 = 0;
        let mut bind_0_seen_revision_for_monitor_main: u64 = 0;
        let scheduler_base_period_ms: u64 = 5;
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
                let __h = health_map.entry("estimator.main".to_string()).or_default();
                __h.name = "estimator.main".to_string();
                __h.lane = "estimator_serial".to_string();
            }
            {
                let __h = health_map.entry("imu_sim.main".to_string()).or_default();
                __h.name = "imu_sim.main".to_string();
                __h.lane = "imu_sim_serial".to_string();
            }
            {
                let __h = health_map.entry("monitor.main".to_string()).or_default();
                __h.name = "monitor.main".to_string();
                __h.lane = "monitor_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                if app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_1_seen_revision_for_estimator_main {
                    bind_1_seen_revision_for_estimator_main = app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
                    scheduler.wake(flowrt::TaskId(1));
                    woke_on_message = true;
                }
                if (app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_2_seen_revision_for_monitor_main || !app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty()) || (app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_0_seen_revision_for_monitor_main || !app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty()) {
                    bind_2_seen_revision_for_monitor_main = app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
                    bind_0_seen_revision_for_monitor_main = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
                    scheduler.wake(flowrt::TaskId(3));
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
                            let __flowrt_component_estimator = app.estimator.clone();
                            let (__flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision) = {
                                let __flowrt_bind_1_snapshot_guard = app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_imu_snapshot_view = __flowrt_bind_1_snapshot_guard.view_at(tick_time_ms);
                                (__flowrt_imu_snapshot_view.as_ref().cloned(), __flowrt_imu_snapshot_view.stale(), __flowrt_bind_1_snapshot_guard.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "estimator.main";
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
                                Self::step_process_main_task_estimator_main(__flowrt_component_estimator, __flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                            let __flowrt_component_imu_sim = app.imu_sim.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "imu_sim.main";
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
                                Self::step_process_main_task_imu_sim_main(__flowrt_component_imu_sim, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                            let __flowrt_component_monitor = app.monitor.clone();
                            let (__flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision) = {
                                let mut __flowrt_bind_2_snapshot_guard = app.bind_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_fifo_read = __flowrt_bind_2_snapshot_guard.pop_at(tick_time_ms);
                                let __flowrt_imu_snapshot_view = __flowrt_fifo_read.view();
                                (__flowrt_imu_snapshot_view.as_ref().cloned(), __flowrt_imu_snapshot_view.stale(), __flowrt_bind_2_snapshot_guard.revision())
                            };
                            let (__flowrt_input_odom_value, __flowrt_input_odom_stale, __flowrt_input_odom_revision) = {
                                let mut __flowrt_bind_0_snapshot_guard = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_fifo_read = __flowrt_bind_0_snapshot_guard.pop_at(tick_time_ms);
                                let __flowrt_odom_snapshot_view = __flowrt_fifo_read.view();
                                (__flowrt_odom_snapshot_view.as_ref().cloned(), __flowrt_odom_snapshot_view.stale(), __flowrt_bind_0_snapshot_guard.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "monitor.main";
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
                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId(3));
                                Self::step_process_main_task_monitor_main(__flowrt_component_monitor, __flowrt_input_imu_value, __flowrt_input_imu_stale, __flowrt_input_imu_revision, __flowrt_input_odom_value, __flowrt_input_odom_stale, __flowrt_input_odom_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                                    let health = health_map.entry("estimator.main".to_string()).or_default();
                                    health.name = "estimator.main".to_string();
                                    health.lane = "estimator_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                flowrt::TaskId(2) => {
                                    let health = health_map.entry("imu_sim.main".to_string()).or_default();
                                    health.name = "imu_sim.main".to_string();
                                    health.lane = "imu_sim_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                flowrt::TaskId(3) => {
                                    let health = health_map.entry("monitor.main".to_string()).or_default();
                                    health.name = "monitor.main".to_string();
                                    health.lane = "monitor_serial".to_string();
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
                            let health = health_map.entry("estimator.main".to_string()).or_default();
                            health.name = "estimator.main".to_string();
                            health.lane = "estimator_serial".to_string();
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
                            let health = health_map.entry("imu_sim.main".to_string()).or_default();
                            health.name = "imu_sim.main".to_string();
                            health.lane = "imu_sim_serial".to_string();
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
                        flowrt::TaskId(3) => {
                            let health = health_map.entry("monitor.main".to_string()).or_default();
                            health.name = "monitor.main".to_string();
                            health.lane = "monitor_serial".to_string();
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
                    if health.lane == "estimator_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(2)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "imu_sim_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(3)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "monitor_serial" {
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
                let next_periodic_deadline_ms = [scheduler.next_deadline_ms(flowrt::TaskId(2))].into_iter().flatten().min();
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
        if monitor_started {
            let stop_status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("monitor", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if estimator_started {
            let stop_status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("estimator", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if imu_sim_started {
            let stop_status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("imu_sim", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if monitor_initialized {
            let shutdown_status = app.monitor.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("monitor", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if estimator_initialized {
            let shutdown_status = app.estimator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("estimator", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if imu_sim_initialized {
            let shutdown_status = app.imu_sim.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("imu_sim", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
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
