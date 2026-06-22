use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::model::{
    IntrospectionHandshake, IntrospectionIdentity, IntrospectionRecorderStart,
    IntrospectionRequest, IntrospectionResponse,
};
use super::paths::{reclaim_stale_socket_path, runtime_socket_path_for_pid};
use super::state::IntrospectionState;

pub(super) const MAX_INTROSPECTION_CLIENT_THREADS: usize = 64;
pub(super) const MAX_INTROSPECTION_OBSERVERS: usize = 32;
pub(super) const INTROSPECTION_INITIAL_REQUEST_TIMEOUT: Duration = Duration::from_millis(100);
const INTROSPECTION_RESPONSE_WRITE_TIMEOUT: Duration = Duration::from_millis(100);

/// 启动一个最小 introspection status 服务。
pub fn spawn_status_server(
    identity: IntrospectionIdentity,
    state: IntrospectionState,
) -> std::io::Result<IntrospectionServer> {
    let handshake = identity.handshake();
    let path = runtime_socket_path_for_pid(handshake.pid);
    spawn_status_server_at(path, handshake, state)
}

/// 在指定路径启动一个最小 introspection status 服务，主要用于测试。
pub fn spawn_status_server_at(
    path: PathBuf,
    handshake: IntrospectionHandshake,
    state: IntrospectionState,
) -> std::io::Result<IntrospectionServer> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    reclaim_stale_socket_path(&path)?;
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let server_path = path.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let active_clients = Arc::new(AtomicUsize::new(0));
    let active_observers = Arc::new(AtomicUsize::new(0));
    let handle = thread::Builder::new()
        .name("flowrt-introspection-server".to_string())
        .spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        let Some(permit) = try_acquire_introspection_client_permit(
                            &active_clients,
                            MAX_INTROSPECTION_CLIENT_THREADS,
                        ) else {
                            let _ = write_error_response(
                                stream,
                                &handshake,
                                "FlowRT introspection connection limit reached",
                            );
                            continue;
                        };
                        let handshake = handshake.clone();
                        let state = state.clone();
                        let active_observers = Arc::clone(&active_observers);
                        let _ = thread::Builder::new()
                            .name("flowrt-introspection-client".to_string())
                            .spawn(move || {
                                let _ = handle_connection(
                                    stream,
                                    &handshake,
                                    &state,
                                    permit,
                                    active_observers,
                                );
                            });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        })?;
    Ok(IntrospectionServer {
        path: server_path,
        handle: Some(handle),
        stop,
    })
}

pub(super) struct IntrospectionClientPermit {
    active_clients: Arc<AtomicUsize>,
}

impl Drop for IntrospectionClientPermit {
    fn drop(&mut self) {
        self.active_clients.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn try_acquire_introspection_client_permit(
    active_clients: &Arc<AtomicUsize>,
    max_clients: usize,
) -> Option<IntrospectionClientPermit> {
    let previous = active_clients.fetch_add(1, Ordering::AcqRel);
    if previous < max_clients {
        Some(IntrospectionClientPermit {
            active_clients: Arc::clone(active_clients),
        })
    } else {
        active_clients.fetch_sub(1, Ordering::AcqRel);
        None
    }
}
/// 已启动的 introspection 服务。
#[derive(Debug)]
pub struct IntrospectionServer {
    path: PathBuf,
    handle: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl IntrospectionServer {
    /// 返回服务 socket 路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for IntrospectionServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = fs::remove_file(&self.path);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_error_response(
    mut stream: UnixStream,
    handshake: &IntrospectionHandshake,
    message: impl Into<String>,
) -> std::io::Result<()> {
    let response = IntrospectionResponse::Error {
        handshake: handshake.clone(),
        message: message.into(),
    };
    stream.write_all(
        serde_json::to_string(&response)
            .map_err(std::io::Error::other)?
            .as_bytes(),
    )?;
    stream.write_all(b"\n")
}

fn handle_connection(
    stream: UnixStream,
    handshake: &IntrospectionHandshake,
    state: &IntrospectionState,
    initial_permit: IntrospectionClientPermit,
    active_observers: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let reader_stream = stream.try_clone()?;
    reader_stream.set_read_timeout(Some(INTROSPECTION_INITIAL_REQUEST_TIMEOUT))?;
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request =
        serde_json::from_str::<IntrospectionRequest>(&line).map_err(std::io::Error::other)?;
    let _client_permit = initial_permit;
    stream.set_write_timeout(Some(INTROSPECTION_RESPONSE_WRITE_TIMEOUT))?;
    match request {
        IntrospectionRequest::Status => {
            let status = state.status();
            let response = IntrospectionResponse::Status {
                handshake: handshake.clone(),
                status,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::SelfDescription => {
            let response = match state.self_description_json() {
                Some(json) => IntrospectionResponse::SelfDescription {
                    handshake: handshake.clone(),
                    json,
                },
                None => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "FlowRT self-description is not registered".to_string(),
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ChannelSnapshot { channel } => {
            let Some(channel) = state.channel_snapshot(&channel) else {
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "unknown FlowRT channel".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let response = IntrospectionResponse::ChannelSnapshot {
                handshake: handshake.clone(),
                channel,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ObserveChannel { channel, .. } => {
            let Some(observer_permit) = try_acquire_introspection_client_permit(
                &active_observers,
                MAX_INTROSPECTION_OBSERVERS,
            ) else {
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "FlowRT introspection observe connection limit reached".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let Some(guard) = state.observe_channel(&channel) else {
                drop(observer_permit);
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "unknown FlowRT channel".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let Some(channel_status) = state.channel_status(&channel) else {
                drop(observer_permit);
                drop(guard);
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "unknown FlowRT channel".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let response = IntrospectionResponse::ObserveReady {
                handshake: handshake.clone(),
                channel: channel_status,
            };
            let mut writer = stream.try_clone()?;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            stream.set_read_timeout(None)?;
            let mut reader = BufReader::new(stream);
            let _observer_permit = observer_permit;
            loop {
                let mut keepalive = String::new();
                match reader.read_line(&mut keepalive) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            drop(guard);
        }
        IntrospectionRequest::ParamList => {
            let response = IntrospectionResponse::ParamList {
                handshake: handshake.clone(),
                params: state.params(),
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ParamGet { name } => {
            let Some(param) = state.param(&name) else {
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: format!("unknown FlowRT parameter `{name}`"),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let response = IntrospectionResponse::ParamValue {
                handshake: handshake.clone(),
                param,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ParamSet { name, value } => {
            let response = match state.set_param_pending(&name, value) {
                Ok(param) => IntrospectionResponse::ParamValue {
                    handshake: handshake.clone(),
                    param,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::BoundaryPublish {
            endpoint,
            payload,
            published_at_ms,
        } => {
            let response = match state.publish_boundary_input(&endpoint, payload, published_at_ms) {
                Ok(boundary) => IntrospectionResponse::BoundaryPublish {
                    handshake: handshake.clone(),
                    boundary,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::OperationCancel { operation_id } => {
            let response = match state.cancel_operation(&operation_id) {
                Ok(operation) => IntrospectionResponse::OperationValue {
                    handshake: handshake.clone(),
                    operation,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::OperationStart {
            operation,
            payload,
            timeout_ms,
            owner,
        } => {
            let response = match state.start_operation(&operation, payload, timeout_ms, owner) {
                Ok(started) => IntrospectionResponse::OperationStarted {
                    handshake: handshake.clone(),
                    started,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::RecorderStart {
            output,
            filters,
            queue_depth,
        } => {
            let recorder = state.start_recorder(IntrospectionRecorderStart {
                output,
                filters,
                queue_depth,
                package: handshake.package.clone(),
                process: handshake.process.clone(),
                runtime_pid: handshake.pid,
                selfdesc_hash: handshake.self_description_hash.clone(),
            });
            state.record_current_diagnostics();
            let response = IntrospectionResponse::RecorderValue {
                handshake: handshake.clone(),
                recorder,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::RecorderStop => {
            let recorder = state.stop_recorder();
            let response = IntrospectionResponse::RecorderValue {
                handshake: handshake.clone(),
                recorder,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::RecorderDrain => {
            let events = state.drain_recorder_events();
            let recorder = state.status().recorder;
            let response = IntrospectionResponse::RecorderEvents {
                handshake: handshake.clone(),
                recorder,
                events,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
    }
    Ok(())
}
