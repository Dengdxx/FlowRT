//! FlowRT runtime 自描述与 live status 的最小 Unix socket 协议。
//!
//! socket 路径只用于发现候选进程；真实身份必须来自连接后的 handshake。协议保持同步、
//! JSON-line 和标准库实现，便于生成 shell 在不引入大型 runtime 依赖的情况下接入。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 当前 introspection 协议版本。
pub const INTROSPECTION_PROTOCOL_VERSION: &str = "0.1";

/// runtime introspection 命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum IntrospectionRequest {
    /// 返回进程 handshake 与当前 live status。
    Status,
    /// 返回指定 channel 的 latest raw ABI snapshot。
    ChannelSnapshot { channel: String },
}

/// runtime introspection 响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum IntrospectionResponse {
    Status {
        handshake: IntrospectionHandshake,
        status: IntrospectionStatus,
    },
    ChannelSnapshot {
        handshake: IntrospectionHandshake,
        channel: IntrospectionChannelSnapshot,
    },
}

/// CLI 连接 socket 后首先验证的进程身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionHandshake {
    pub protocol_version: String,
    pub pid: u32,
    pub started_at_unix_ms: u64,
    pub self_description_hash: String,
    pub package: String,
    pub process: String,
    pub runtime: String,
}

/// 运行态状态快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionStatus {
    pub tick_count: u64,
    pub channels: Vec<IntrospectionChannelStatus>,
}

/// 单个 channel 的运行态摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionChannelStatus {
    pub name: String,
    pub message_type: String,
    pub published_count: u64,
    pub last_payload_len: Option<usize>,
}

/// 单个 channel 的 latest raw ABI snapshot。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionChannelSnapshot {
    pub name: String,
    pub message_type: String,
    pub published_count: u64,
    pub payload: Option<Vec<u8>>,
    pub published_at_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct ChannelState {
    message_type: String,
    published_count: u64,
    payload: Option<Vec<u8>>,
    published_at_ms: Option<u64>,
}

/// runtime shell 可共享更新的 introspection live 状态。
#[derive(Debug, Clone, Default)]
pub struct IntrospectionState {
    inner: Arc<Mutex<IntrospectionStateInner>>,
}

#[derive(Debug, Default)]
struct IntrospectionStateInner {
    tick_count: u64,
    channels: BTreeMap<String, ChannelState>,
}

impl IntrospectionState {
    /// 构造空 live 状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 预注册一个 channel，使其在尚未发布样本时也出现在 status 中。
    pub fn register_channel(&self, name: impl Into<String>, message_type: impl Into<String>) {
        let name = name.into();
        let mut inner = self
            .inner
            .lock()
            .expect("introspection state mutex poisoned");
        inner.channels.entry(name).or_insert_with(|| ChannelState {
            message_type: message_type.into(),
            published_count: 0,
            payload: None,
            published_at_ms: None,
        });
    }

    /// 增加 scheduler tick 计数。
    pub fn record_tick(&self) {
        let mut inner = self
            .inner
            .lock()
            .expect("introspection state mutex poisoned");
        inner.tick_count = inner.tick_count.saturating_add(1);
    }

    /// 记录 channel 发布的 latest raw ABI payload。
    pub fn record_channel_publish<T: Copy>(
        &self,
        name: impl Into<String>,
        message_type: impl Into<String>,
        value: &T,
        published_at_ms: Option<u64>,
    ) {
        self.record_channel_publish_bytes(name, message_type, bytes_of(value), published_at_ms);
    }

    /// 记录 channel 发布的 raw ABI bytes。
    pub fn record_channel_publish_bytes(
        &self,
        name: impl Into<String>,
        message_type: impl Into<String>,
        payload: Vec<u8>,
        published_at_ms: Option<u64>,
    ) {
        let name = name.into();
        let message_type = message_type.into();
        let mut inner = self
            .inner
            .lock()
            .expect("introspection state mutex poisoned");
        let channel = inner.channels.entry(name).or_insert_with(|| ChannelState {
            message_type: message_type.clone(),
            published_count: 0,
            payload: None,
            published_at_ms: None,
        });
        channel.message_type = message_type;
        channel.published_count = channel.published_count.saturating_add(1);
        channel.payload = Some(payload);
        channel.published_at_ms = published_at_ms;
    }

    /// 返回当前 status 快照。
    pub fn status(&self) -> IntrospectionStatus {
        let inner = self
            .inner
            .lock()
            .expect("introspection state mutex poisoned");
        IntrospectionStatus {
            tick_count: inner.tick_count,
            channels: inner
                .channels
                .iter()
                .map(|(name, channel)| IntrospectionChannelStatus {
                    name: name.clone(),
                    message_type: channel.message_type.clone(),
                    published_count: channel.published_count,
                    last_payload_len: channel.payload.as_ref().map(Vec::len),
                })
                .collect(),
        }
    }

    /// 返回指定 channel 的 raw ABI snapshot。
    pub fn channel_snapshot(&self, name: &str) -> Option<IntrospectionChannelSnapshot> {
        let inner = self
            .inner
            .lock()
            .expect("introspection state mutex poisoned");
        inner
            .channels
            .get(name)
            .map(|channel| IntrospectionChannelSnapshot {
                name: name.to_string(),
                message_type: channel.message_type.clone(),
                published_count: channel.published_count,
                payload: channel.payload.clone(),
                published_at_ms: channel.published_at_ms,
            })
    }
}

/// 生成 handshake 的输入元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrospectionIdentity {
    pub self_description_hash: String,
    pub package: String,
    pub process: String,
    pub runtime: String,
}

impl IntrospectionIdentity {
    /// 构造当前进程的 handshake。
    pub fn handshake(&self) -> IntrospectionHandshake {
        IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: std::process::id(),
            started_at_unix_ms: unix_time_ms(),
            self_description_hash: self.self_description_hash.clone(),
            package: self.package.clone(),
            process: self.process.clone(),
            runtime: self.runtime.clone(),
        }
    }
}

/// 返回当前用户 runtime socket 目录。
///
/// 优先使用 `$XDG_RUNTIME_DIR/flowrt`；没有时 fallback 到 `/tmp/flowrt.<uid>`，避免不同用户
/// 的同名 PID socket 互相污染。
pub fn runtime_socket_dir() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("flowrt");
    }
    PathBuf::from(format!("/tmp/flowrt.{}", current_uid()))
}

/// 返回当前进程默认 runtime socket 路径。
pub fn runtime_socket_path_for_pid(pid: u32) -> PathBuf {
    runtime_socket_dir().join(format!("{pid}.sock"))
}

/// 扫描当前用户 runtime socket 目录中的 FlowRT socket 候选。
pub fn discover_runtime_sockets() -> std::io::Result<Vec<PathBuf>> {
    let dir = runtime_socket_dir();
    let mut sockets = Vec::new();
    match fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "sock")
                {
                    sockets.push(path);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    sockets.sort();
    Ok(sockets)
}

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
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let server_path = path.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    let _ = handle_connection(stream, &handshake, &state);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    Ok(IntrospectionServer {
        path: server_path,
        handle: Some(handle),
        stop,
    })
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

/// 向 introspection socket 请求 status。
pub fn request_status(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::Status)
}

/// 向 introspection socket 请求 channel snapshot。
pub fn request_channel_snapshot(
    path: &Path,
    channel: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::ChannelSnapshot {
            channel: channel.into(),
        },
    )
}

fn request(path: &Path, request: &IntrospectionRequest) -> std::io::Result<IntrospectionResponse> {
    let mut stream = UnixStream::connect(path)?;
    let request = serde_json::to_string(request).map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

fn handle_connection(
    stream: UnixStream,
    handshake: &IntrospectionHandshake,
    state: &IntrospectionState,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request =
        serde_json::from_str::<IntrospectionRequest>(&line).map_err(std::io::Error::other)?;
    match request {
        IntrospectionRequest::Status => {
            let response = IntrospectionResponse::Status {
                handshake: handshake.clone(),
                status: state.status(),
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
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "unknown FlowRT channel",
                ));
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
    }
    Ok(())
}

fn bytes_of<T: Copy>(value: &T) -> Vec<u8> {
    let mut bytes = vec![0u8; std::mem::size_of::<T>()];
    unsafe {
        // T: Copy 且只读取对象表示；这些 bytes 仅用于诊断快照，不反序列化成新所有权值。
        std::ptr::copy_nonoverlapping(
            (value as *const T).cast::<u8>(),
            bytes.as_mut_ptr(),
            bytes.len(),
        );
    }
    bytes
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe { libc_getuid() }
}

#[cfg(unix)]
unsafe extern "C" {
    fn getuid() -> u32;
}

#[cfg(unix)]
unsafe fn libc_getuid() -> u32 {
    unsafe { getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_uses_pid_name_under_runtime_dir() {
        let dir = runtime_socket_dir();
        let path = runtime_socket_path_for_pid(1234);

        assert_eq!(path, dir.join("1234.sock"));
    }

    #[test]
    fn status_server_returns_handshake_and_snapshot() {
        let root =
            std::env::temp_dir().join(format!("flowrt-introspection-test-{}", std::process::id()));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        for _ in 0..7 {
            state.record_tick();
        }
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![1u8; 48], Some(7));
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![2u8; 48], Some(8));
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![3u8; 48], Some(9));

        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
            .expect("server should start");
        let IntrospectionResponse::Status {
            handshake: response_handshake,
            status,
        } = request_status(server.path()).expect("status request should succeed")
        else {
            panic!("status request returned wrong response")
        };

        assert_eq!(response_handshake, handshake);
        assert_eq!(status.tick_count, 7);
        assert_eq!(
            status.channels,
            vec![IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 3,
                last_payload_len: Some(48),
            }]
        );

        state.record_tick();
        let IntrospectionResponse::Status { status, .. } =
            request_status(server.path()).expect("second status request should succeed")
        else {
            panic!("status request returned wrong response")
        };
        assert_eq!(status.tick_count, 8);

        let IntrospectionResponse::ChannelSnapshot { channel, .. } =
            request_channel_snapshot(server.path(), "source.imu_to_sink.imu")
                .expect("snapshot request should succeed")
        else {
            panic!("snapshot request returned wrong response")
        };
        assert_eq!(channel.message_type, "Imu");
        assert_eq!(channel.published_count, 3);
        assert_eq!(channel.payload, Some(vec![3u8; 48]));
        assert_eq!(channel.published_at_ms, Some(9));

        drop(server);
        let _ = fs::remove_dir_all(root);
    }
}
