// FlowRT 管理产物。不要手工修改。

use crate::components::*;
use crate::messages::*;
use crate::selfdesc;
use crate::user;

const PACKAGE_NAME: &str = "bounded_channel_iox2_rust";

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
    source: std::sync::Arc<std::sync::Mutex<Box<dyn Source + Send>>>,
    sink: std::sync::Arc<std::sync::Mutex<Box<dyn Sink + Send>>>,
    bind_0: std::sync::Arc<std::sync::Mutex<flowrt::iox2::Iox2FramePubSub<Packet, 60>>>,
    introspection_probe_bind_0: std::sync::OnceLock<flowrt::IntrospectionChannelProbe>,
}

impl App {
    pub fn new(
        source: Box<dyn Source + Send>,
        sink: Box<dyn Sink + Send>,
    ) -> Self {
        let mut startup_status = flowrt::Status::Ok;
        let source = std::sync::Arc::new(std::sync::Mutex::new(source));
        let sink = std::sync::Arc::new(std::sync::Mutex::new(sink));
        Self {
            source: source.clone(),
            sink: sink.clone(),
            bind_0: std::sync::Arc::new(std::sync::Mutex::new(match flowrt::iox2::Iox2FramePubSub::open_with_config("FlowRT/bounded_channel_iox2_rust/default/bind_0/source_packet_to_sink_packet", flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(flowrt::StaleConfig::new(None, flowrt::StalePolicy::Warn))) {
                Ok(channel) => channel,
                Err(error) => {
                    eprintln!("FlowRT: failed to open iox2 channel {}: {error}", "FlowRT/bounded_channel_iox2_rust/default/bind_0/source_packet_to_sink_packet");
                    startup_status = flowrt::Status::Error;
                    flowrt::iox2::Iox2FramePubSub::unavailable("FlowRT/bounded_channel_iox2_rust/default/bind_0/source_packet_to_sink_packet", flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(flowrt::StaleConfig::new(None, flowrt::StalePolicy::Warn)), error.to_string())
                }
            })),
            introspection_probe_bind_0: std::sync::OnceLock::new(),
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
                let __h = health_map.entry("source.main".to_string()).or_default();
                __h.name = "source.main".to_string();
                __h.lane = "source_serial".to_string();
            }
            let mut packet = flowrt::Output::<Packet>::new();
            match self.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut packet) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
            if let Some(value) = packet.as_ref().cloned() {
                let mut __flowrt_route = self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let __flowrt_publish_result = __flowrt_route.publish_at(value.clone(), tick_time_ms);
                let __flowrt_route_health = __flowrt_route.health();
                drop(__flowrt_route);
                introspection_state.record_route_backend_health("source.packet_to_sink.packet", __flowrt_route_health);
                if let Err(error) = __flowrt_publish_result {
                    let __flowrt_error_kind = flowrt::transport_error_kind(&error);
                    introspection_state.record_route_transport_error("source.packet_to_sink.packet", flowrt::OverflowPolicy::DropOldest, __flowrt_error_kind, error.to_string());
                    return flowrt::Status::Error;
                }
                introspection_state.record_route_publish("source.packet_to_sink.packet", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_0_probe) = self.introspection_probe_bind_0.get() {
                    record_introspection_publish_frame(&introspection_state, "source.packet_to_sink.packet", "Packet", introspection_probe_bind_0_probe, &value, tick_time_ms);
                }
            }
        }
        {
            let __flowrt_bind_0_guard = self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let packet = __flowrt_bind_0_guard.cached_latest_at(tick_time_ms);
            let __flowrt_packet_revision = __flowrt_bind_0_guard.revision();
            record_introspection_input_read(&introspection_state, "sink.main.packet", "sink.main", "packet", "source.packet_to_sink.packet", "Packet", &packet, __flowrt_packet_revision, tick_time_ms);
            if packet.stale() {
                health_map.entry("sink.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("sink.main".to_string()).or_default();
                __h.name = "sink.main".to_string();
                __h.lane = "sink_serial".to_string();
            }
            if packet.present() {
                match self.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(packet) {
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
    fn step_task_sink_main(
        __flowrt_component_sink: std::sync::Arc<std::sync::Mutex<Box<dyn Sink + Send>>>,
        __flowrt_input_packet_value: Option<Packet>,
        __flowrt_input_packet_stale: bool,
        __flowrt_input_packet_revision: u64,
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
            let packet = flowrt::Latest::new(__flowrt_input_packet_value.as_ref(), __flowrt_input_packet_stale);
            let __flowrt_packet_revision = __flowrt_input_packet_revision;
            record_introspection_input_read(&introspection_state, "sink.main.packet", "sink.main", "packet", "source.packet_to_sink.packet", "Packet", &packet, __flowrt_packet_revision, tick_time_ms);
            if packet.stale() {
                health_map.entry("sink.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("sink.main".to_string()).or_default();
                __h.name = "sink.main".to_string();
                __h.lane = "sink_serial".to_string();
            }
            if packet.present() {
                match __flowrt_component_sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(packet) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
                }
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_task_source_main(
        __flowrt_component_source: std::sync::Arc<std::sync::Mutex<Box<dyn Source + Send>>>,
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
                let __h = health_map.entry("source.main".to_string()).or_default();
                __h.name = "source.main".to_string();
                __h.lane = "source_serial".to_string();
            }
            let mut packet = flowrt::Output::<Packet>::new();
            match __flowrt_component_source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut packet) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
            if let Some(value) = packet.as_ref().cloned() {
                let value = value.clone();
                __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, _health_map| {
                let mut __flowrt_route = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let __flowrt_publish_result = __flowrt_route.publish_at(value.clone(), tick_time_ms);
                let __flowrt_route_health = __flowrt_route.health();
                drop(__flowrt_route);
                introspection_state.record_route_backend_health("source.packet_to_sink.packet", __flowrt_route_health);
                if let Err(error) = __flowrt_publish_result {
                    let __flowrt_error_kind = flowrt::transport_error_kind(&error);
                    introspection_state.record_route_transport_error("source.packet_to_sink.packet", flowrt::OverflowPolicy::DropOldest, __flowrt_error_kind, error.to_string());
                    return flowrt::Status::Error;
                }
                introspection_state.record_route_publish("source.packet_to_sink.packet", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_0_probe) = app.introspection_probe_bind_0.get() {
                    record_introspection_publish_frame(&introspection_state, "source.packet_to_sink.packet", "Packet", introspection_probe_bind_0_probe, &value, tick_time_ms);
                }
                    flowrt::Status::Ok
                }));
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_sink_proc(
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
            let __flowrt_bind_0_guard = self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let packet = __flowrt_bind_0_guard.cached_latest_at(tick_time_ms);
            let __flowrt_packet_revision = __flowrt_bind_0_guard.revision();
            record_introspection_input_read(&introspection_state, "sink.main.packet", "sink.main", "packet", "source.packet_to_sink.packet", "Packet", &packet, __flowrt_packet_revision, tick_time_ms);
            if packet.stale() {
                health_map.entry("sink.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("sink.main".to_string()).or_default();
                __h.name = "sink.main".to_string();
                __h.lane = "sink_serial".to_string();
            }
            if packet.present() {
                match self.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(packet) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::Status::Retry,
                    flowrt::Status::Error => return flowrt::Status::Error,
                }
            }
        }
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_sink_proc_startup(
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
    fn step_process_sink_proc_shutdown(
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
    fn step_process_sink_proc_task_sink_main(
        __flowrt_component_sink: std::sync::Arc<std::sync::Mutex<Box<dyn Sink + Send>>>,
        __flowrt_input_packet_value: Option<Packet>,
        __flowrt_input_packet_stale: bool,
        __flowrt_input_packet_revision: u64,
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
            let packet = flowrt::Latest::new(__flowrt_input_packet_value.as_ref(), __flowrt_input_packet_stale);
            let __flowrt_packet_revision = __flowrt_input_packet_revision;
            record_introspection_input_read(&introspection_state, "sink.main.packet", "sink.main", "packet", "source.packet_to_sink.packet", "Packet", &packet, __flowrt_packet_revision, tick_time_ms);
            if packet.stale() {
                health_map.entry("sink.main".to_string()).or_default().stale_input += 1;
            }
            {
                let __h = health_map.entry("sink.main".to_string()).or_default();
                __h.name = "sink.main".to_string();
                __h.lane = "sink_serial".to_string();
            }
            if packet.present() {
                match __flowrt_component_sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(packet) {
                    flowrt::Status::Ok => {}
                    flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                    flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
                }
            }
        }
        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)
    }
    #[allow(dead_code)]
    fn step_process_source_proc(
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
                let __h = health_map.entry("source.main".to_string()).or_default();
                __h.name = "source.main".to_string();
                __h.lane = "source_serial".to_string();
            }
            let mut packet = flowrt::Output::<Packet>::new();
            match self.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut packet) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::Status::Retry,
                flowrt::Status::Error => return flowrt::Status::Error,
            }
            if let Some(value) = packet.as_ref().cloned() {
                let mut __flowrt_route = self.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let __flowrt_publish_result = __flowrt_route.publish_at(value.clone(), tick_time_ms);
                let __flowrt_route_health = __flowrt_route.health();
                drop(__flowrt_route);
                introspection_state.record_route_backend_health("source.packet_to_sink.packet", __flowrt_route_health);
                if let Err(error) = __flowrt_publish_result {
                    let __flowrt_error_kind = flowrt::transport_error_kind(&error);
                    introspection_state.record_route_transport_error("source.packet_to_sink.packet", flowrt::OverflowPolicy::DropOldest, __flowrt_error_kind, error.to_string());
                    return flowrt::Status::Error;
                }
                introspection_state.record_route_publish("source.packet_to_sink.packet", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_0_probe) = self.introspection_probe_bind_0.get() {
                    record_introspection_publish_frame(&introspection_state, "source.packet_to_sink.packet", "Packet", introspection_probe_bind_0_probe, &value, tick_time_ms);
                }
            }
        }
        flowrt::Status::Ok
    }
    #[allow(dead_code)]
    fn step_process_source_proc_startup(
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
    fn step_process_source_proc_shutdown(
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
    fn step_process_source_proc_task_source_main(
        __flowrt_component_source: std::sync::Arc<std::sync::Mutex<Box<dyn Source + Send>>>,
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
                let __h = health_map.entry("source.main".to_string()).or_default();
                __h.name = "source.main".to_string();
                __h.lane = "source_serial".to_string();
            }
            let mut packet = flowrt::Output::<Packet>::new();
            match __flowrt_component_source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut packet) {
                flowrt::Status::Ok => {}
                flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),
                flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),
            }
            if let Some(value) = packet.as_ref().cloned() {
                let value = value.clone();
                __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, _health_map| {
                let mut __flowrt_route = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let __flowrt_publish_result = __flowrt_route.publish_at(value.clone(), tick_time_ms);
                let __flowrt_route_health = __flowrt_route.health();
                drop(__flowrt_route);
                introspection_state.record_route_backend_health("source.packet_to_sink.packet", __flowrt_route_health);
                if let Err(error) = __flowrt_publish_result {
                    let __flowrt_error_kind = flowrt::transport_error_kind(&error);
                    introspection_state.record_route_transport_error("source.packet_to_sink.packet", flowrt::OverflowPolicy::DropOldest, __flowrt_error_kind, error.to_string());
                    return flowrt::Status::Error;
                }
                introspection_state.record_route_publish("source.packet_to_sink.packet", Some(tick_time_ms));
                scheduler_events.notify_data();
                if let Some(introspection_probe_bind_0_probe) = app.introspection_probe_bind_0.get() {
                    record_introspection_publish_frame(&introspection_state, "source.packet_to_sink.packet", "Packet", introspection_probe_bind_0_probe, &value, tick_time_ms);
                }
                    flowrt::Status::Ok
                }));
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
        app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let introspection_probe_bind_0 = register_introspection_channel(&introspection_state, "source.packet_to_sink.packet", "Packet", None);
        let _ = app.introspection_probe_bind_0.set(introspection_probe_bind_0);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "source.packet_to_sink.packet".to_string(),
            from: "source.packet".to_string(),
            to: "sink.packet".to_string(),
            message_type: "Packet".to_string(),
            backend: "iox2".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "sink.main".to_string(),
            input: "packet".to_string(),
            channel: "source.packet_to_sink.packet".to_string(),
            message_type: "Packet".to_string(),
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
        let mut source_initialized = false;
        let mut source_started = false;
        introspection_state.record_lifecycle_state("source", flowrt::LifecycleState::Uninitialized);
        let mut sink_initialized = false;
        let mut sink_started = false;
        introspection_state.record_lifecycle_state("sink", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            source_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("source", if source_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            sink_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("sink", if sink_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && source_initialized {
            status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            source_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("source", if source_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && sink_initialized {
            status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            sink_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("sink", if sink_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "sink_serial";
        scheduler.add_lane(flowrt::LaneId(2), flowrt::LaneKind::Serial);
        let _ = "source_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(2), period_ms: 5 });
        scheduler.wake(flowrt::TaskId(2));
        let mut bind_0_seen_revision_for_sink_main: u64 = 0;
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
                let __h = health_map.entry("sink.main".to_string()).or_default();
                __h.name = "sink.main".to_string();
                __h.lane = "sink_serial".to_string();
            }
            {
                let __h = health_map.entry("source.main".to_string()).or_default();
                __h.name = "source.main".to_string();
                __h.lane = "source_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                let _ = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms);
                if app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_0_seen_revision_for_sink_main {
                    bind_0_seen_revision_for_sink_main = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
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
                            let __flowrt_component_sink = app.sink.clone();
                            let (__flowrt_input_packet_value, __flowrt_input_packet_stale, __flowrt_input_packet_revision) = {
                                let __flowrt_bind_0_snapshot_guard = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_packet_snapshot_view = __flowrt_bind_0_snapshot_guard.cached_latest_at(tick_time_ms);
                                (__flowrt_packet_snapshot_view.as_ref().cloned(), __flowrt_packet_snapshot_view.stale(), __flowrt_bind_0_snapshot_guard.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "sink.main";
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
                                Self::step_task_sink_main(__flowrt_component_sink, __flowrt_input_packet_value, __flowrt_input_packet_stale, __flowrt_input_packet_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                            let __flowrt_component_source = app.source.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "source.main";
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
                                Self::step_task_source_main(__flowrt_component_source, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                                    let health = health_map.entry("sink.main".to_string()).or_default();
                                    health.name = "sink.main".to_string();
                                    health.lane = "sink_serial".to_string();
                                    health.inflight = true;
                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);
                                    health.observed_time_ms = Some(admission.observed_time_ms);
                                    health.lateness_ms = Some(admission.lateness_ms);
                                    health.missed_periods = Some(admission.missed_periods);
                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));
                                }
                                flowrt::TaskId(2) => {
                                    let health = health_map.entry("source.main".to_string()).or_default();
                                    health.name = "source.main".to_string();
                                    health.lane = "source_serial".to_string();
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
                            let health = health_map.entry("sink.main".to_string()).or_default();
                            health.name = "sink.main".to_string();
                            health.lane = "sink_serial".to_string();
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
                            let health = health_map.entry("source.main".to_string()).or_default();
                            health.name = "source.main".to_string();
                            health.lane = "source_serial".to_string();
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
                    if health.lane == "sink_serial" {
                        health.fairness_violations += 1;
                    }
                }
            }
            if scheduler.lane_starvation_ticks(flowrt::LaneId(2)) > FAIRNESS_STARVATION_THRESHOLD {
                for health in health_map.values_mut() {
                    if health.lane == "source_serial" {
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
        if sink_started {
            let stop_status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("sink", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if source_started {
            let stop_status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("source", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if sink_initialized {
            let shutdown_status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("sink", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
        }
        if source_initialized {
            let shutdown_status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("source", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
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
            "sink_proc" => self.run_process_sink_proc(backend, run_ticks),
            "source_proc" => self.run_process_source_proc(backend, run_ticks),
            _ => flowrt::Status::Error,
        }
    }
    fn run_process_sink_proc(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {
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
        app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let introspection_probe_bind_0 = register_introspection_channel(&introspection_state, "source.packet_to_sink.packet", "Packet", None);
        let _ = app.introspection_probe_bind_0.set(introspection_probe_bind_0);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "source.packet_to_sink.packet".to_string(),
            from: "source.packet".to_string(),
            to: "sink.packet".to_string(),
            message_type: "Packet".to_string(),
            backend: "iox2".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {
            task: "sink.main".to_string(),
            input: "packet".to_string(),
            channel: "source.packet_to_sink.packet".to_string(),
            message_type: "Packet".to_string(),
            ..Default::default()
        });
        let _introspection_server = flowrt::spawn_status_server(
            flowrt::IntrospectionIdentity {
                self_description_hash: selfdesc::self_description_hash().to_string(),
                package: PACKAGE_NAME.to_string(),
                process: "sink_proc".to_string(),
                runtime: "rust".to_string(),
            },
            introspection_state.clone(),
        )
        .ok();
        let mut sink_initialized = false;
        let mut sink_started = false;
        introspection_state.record_lifecycle_state("sink", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            sink_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("sink", if sink_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && sink_initialized {
            status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            sink_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("sink", if sink_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_sink_proc_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "sink_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        let mut bind_0_seen_revision_for_sink_main: u64 = 0;
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
                let __h = health_map.entry("sink.main".to_string()).or_default();
                __h.name = "sink.main".to_string();
                __h.lane = "sink_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let mut woke_on_message = false;
                let _ = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms);
                if app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_0_seen_revision_for_sink_main {
                    bind_0_seen_revision_for_sink_main = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();
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
                            let __flowrt_component_sink = app.sink.clone();
                            let (__flowrt_input_packet_value, __flowrt_input_packet_stale, __flowrt_input_packet_revision) = {
                                let __flowrt_bind_0_snapshot_guard = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                let __flowrt_packet_snapshot_view = __flowrt_bind_0_snapshot_guard.cached_latest_at(tick_time_ms);
                                (__flowrt_packet_snapshot_view.as_ref().cloned(), __flowrt_packet_snapshot_view.stale(), __flowrt_bind_0_snapshot_guard.revision())
                            };
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "sink.main";
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
                                Self::step_process_sink_proc_task_sink_main(__flowrt_component_sink, __flowrt_input_packet_value, __flowrt_input_packet_stale, __flowrt_input_packet_revision, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                                    let health = health_map.entry("sink.main".to_string()).or_default();
                                    health.name = "sink.main".to_string();
                                    health.lane = "sink_serial".to_string();
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
                            let health = health_map.entry("sink.main".to_string()).or_default();
                            health.name = "sink.main".to_string();
                            health.lane = "sink_serial".to_string();
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
                    if health.lane == "sink_serial" {
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
            status = app.step_process_sink_proc_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if sink_started {
            let stop_status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("sink", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if sink_initialized {
            let shutdown_status = app.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("sink", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
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
    fn run_process_source_proc(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {
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
        app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());
        introspection_state.set_self_description_json(selfdesc::self_description_json());
        let introspection_probe_bind_0 = register_introspection_channel(&introspection_state, "source.packet_to_sink.packet", "Packet", None);
        let _ = app.introspection_probe_bind_0.set(introspection_probe_bind_0);
        introspection_state.register_route(flowrt::IntrospectionRouteStatus {
            name: "source.packet_to_sink.packet".to_string(),
            from: "source.packet".to_string(),
            to: "sink.packet".to_string(),
            message_type: "Packet".to_string(),
            backend: "iox2".to_string(),
            selected_reason: "profile_default".to_string(),
            ..Default::default()
        });
        let _introspection_server = flowrt::spawn_status_server(
            flowrt::IntrospectionIdentity {
                self_description_hash: selfdesc::self_description_hash().to_string(),
                package: PACKAGE_NAME.to_string(),
                process: "source_proc".to_string(),
                runtime: "rust".to_string(),
            },
            introspection_state.clone(),
        )
        .ok();
        let mut source_initialized = false;
        let mut source_started = false;
        introspection_state.record_lifecycle_state("source", flowrt::LifecycleState::Uninitialized);
        if status == flowrt::Status::Ok {
            status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_init(&mut lifecycle_context);
            source_initialized = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("source", if source_initialized { flowrt::LifecycleState::Initialized } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok && source_initialized {
            status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_start(&mut lifecycle_context);
            source_started = status == flowrt::Status::Ok;
            introspection_state.record_lifecycle_state("source", if source_started { flowrt::LifecycleState::Running } else { flowrt::LifecycleState::Faulted });
        }
        if status == flowrt::Status::Ok {
            status = app.step_process_source_proc_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        let mut scheduler = flowrt::DeterministicExecutor::new(1);
        let worker_pool = flowrt::WorkerPool::new(1);
        scheduler.add_lane(flowrt::LaneId(1), flowrt::LaneKind::Serial);
        let _ = "source_serial";
        scheduler.add_task(flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 });
        scheduler.add_periodic(flowrt::PeriodicSpec { task: flowrt::TaskId(1), period_ms: 5 });
        scheduler.wake(flowrt::TaskId(1));
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
                let __h = health_map.entry("source.main".to_string()).or_default();
                __h.name = "source.main".to_string();
                __h.lane = "source_serial".to_string();
            }
            introspection_state.record_tick_at(tick_time_ms, clock_source);
            loop {
                observed_data_generation = scheduler_events.data_generation();
                let woke_on_message = false;
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
                            let __flowrt_component_source = app.source.clone();
                            let introspection_state = introspection_state.clone();
                            let scheduler_events = scheduler_events.clone();
                            let task_health_from_worker = task_health_from_workers.clone();
                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {
                            let task_name = "source.main";
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
                                Self::step_process_source_proc_task_source_main(__flowrt_component_source, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)
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
                                    let health = health_map.entry("source.main".to_string()).or_default();
                                    health.name = "source.main".to_string();
                                    health.lane = "source_serial".to_string();
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
                            let health = health_map.entry("source.main".to_string()).or_default();
                            health.name = "source.main".to_string();
                            health.lane = "source_serial".to_string();
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
                    if health.lane == "source_serial" {
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
            status = app.step_process_source_proc_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());
        }
        if source_started {
            let stop_status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_stop(&mut lifecycle_context);
            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("source", if stop_status == flowrt::Status::Ok { flowrt::LifecycleState::Stopped } else { flowrt::LifecycleState::Faulted });
        }
        if source_initialized {
            let shutdown_status = app.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_shutdown(&mut lifecycle_context);
            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {
                status = flowrt::Status::Error;
            }
            introspection_state.record_lifecycle_state("source", if shutdown_status == flowrt::Status::Ok { flowrt::LifecycleState::ShutDown } else { flowrt::LifecycleState::Faulted });
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
