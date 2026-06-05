//! FlowRT runtime 自描述与 live status 的最小 Unix socket 协议。
//!
//! socket 路径只用于发现候选进程；真实身份必须来自连接后的 handshake。协议保持同步、
//! JSON-line 和标准库实现，便于生成 shell 在不引入大型 runtime 依赖的情况下接入。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicU64, Ordering},
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
    /// 返回编译期 self-description JSON。
    SelfDescription,
    /// 返回指定 channel 的 latest raw ABI snapshot。
    ChannelSnapshot { channel: String },
    /// 为指定 channel 建立连接作用域的数据面观测。
    ObserveChannel {
        channel: String,
        #[serde(default)]
        mode: Option<String>,
    },
    /// 返回当前进程内可观察的参数列表。
    ParamList,
    /// 返回指定参数的当前值与 pending 值。
    ParamGet { name: String },
    /// 设置指定参数的 pending 值，由 generated shell 在 tick 边界应用。
    ParamSet {
        name: String,
        value: serde_json::Value,
    },
}

/// runtime introspection 响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum IntrospectionResponse {
    Status {
        handshake: IntrospectionHandshake,
        status: IntrospectionStatus,
    },
    SelfDescription {
        handshake: IntrospectionHandshake,
        json: String,
    },
    ChannelSnapshot {
        handshake: IntrospectionHandshake,
        channel: IntrospectionChannelSnapshot,
    },
    ObserveReady {
        handshake: IntrospectionHandshake,
        channel: IntrospectionChannelStatus,
    },
    ParamList {
        handshake: IntrospectionHandshake,
        params: Vec<IntrospectionParamStatus>,
    },
    ParamValue {
        handshake: IntrospectionHandshake,
        param: IntrospectionParamStatus,
    },
    Error {
        handshake: IntrospectionHandshake,
        message: String,
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
    #[serde(default)]
    pub active_observers: u64,
    #[serde(default)]
    pub dropped_samples: u64,
}

/// 单个 channel 的 latest raw ABI snapshot。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionChannelSnapshot {
    pub published_count: u64,
    pub payload: Option<Vec<u8>>,
    pub published_at_ms: Option<u64>,
}

/// 数据面 probe 记录结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrospectionProbeRecord {
    pub recorded: bool,
    pub dropped: bool,
}

/// generated shell 注册到 runtime 控制面的参数 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionParamSchema {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub update: String,
    pub current: serde_json::Value,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub choices: Vec<serde_json::Value>,
}

/// runtime 参数状态快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionParamStatus {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub update: String,
    pub current: serde_json::Value,
    pub pending: Option<serde_json::Value>,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub choices: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct ChannelState {
    message_type: String,
    probe: IntrospectionChannelProbe,
}

#[derive(Debug)]
struct ChannelProbeInner {
    observer_count: AtomicU64,
    dropped_samples: AtomicU64,
    latest: Mutex<ChannelProbeLatest>,
}

#[derive(Debug, Default)]
struct ChannelProbeLatest {
    published_count: u64,
    payload: Option<Vec<u8>>,
    published_at_ms: Option<u64>,
    max_payload_len: Option<usize>,
}

/// 单个 channel 的按需数据面 probe。
#[derive(Debug, Clone)]
pub struct IntrospectionChannelProbe {
    inner: Arc<ChannelProbeInner>,
}

impl Default for IntrospectionChannelProbe {
    fn default() -> Self {
        Self::new(None)
    }
}

impl IntrospectionChannelProbe {
    fn new(max_payload_len: Option<usize>) -> Self {
        let payload = max_payload_len.map(Vec::with_capacity);
        Self {
            inner: Arc::new(ChannelProbeInner {
                observer_count: AtomicU64::new(0),
                dropped_samples: AtomicU64::new(0),
                latest: Mutex::new(ChannelProbeLatest {
                    published_count: 0,
                    payload,
                    published_at_ms: None,
                    max_payload_len,
                }),
            }),
        }
    }

    /// 判断当前 channel 是否有 active echo observer。
    pub fn enabled(&self) -> bool {
        self.inner.observer_count.load(Ordering::Acquire) != 0
    }

    /// active observer 数量。
    pub fn active_count(&self) -> u64 {
        self.inner.observer_count.load(Ordering::Acquire)
    }

    /// 被 probe 丢弃的观测样本数量。
    pub fn dropped_samples(&self) -> u64 {
        self.inner.dropped_samples.load(Ordering::Acquire)
    }

    /// 建立一个 observer guard；guard drop 后自动关闭 probe。
    pub fn observe(&self) -> IntrospectionObserverGuard {
        self.inner.observer_count.fetch_add(1, Ordering::AcqRel);
        IntrospectionObserverGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    /// 非阻塞记录观测 payload。无观察者时只做原子读取；锁繁忙或超出上界时丢弃观测样本。
    pub fn try_record_bytes(
        &self,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> IntrospectionProbeRecord {
        if !self.enabled() {
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: false,
            };
        }
        let Ok(mut latest) = self.inner.latest.try_lock() else {
            self.inner.dropped_samples.fetch_add(1, Ordering::Relaxed);
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            };
        };
        if latest
            .max_payload_len
            .is_some_and(|max_payload_len| payload.len() > max_payload_len)
        {
            self.inner.dropped_samples.fetch_add(1, Ordering::Relaxed);
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            };
        }
        let max_payload_len = latest.max_payload_len;
        let buffer = latest.payload.get_or_insert_with(Vec::new);
        if let Some(max_payload_len) = max_payload_len
            && buffer.capacity() < max_payload_len
        {
            self.inner.dropped_samples.fetch_add(1, Ordering::Relaxed);
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            };
        }
        buffer.clear();
        buffer.extend_from_slice(payload);
        latest.published_count = latest.published_count.saturating_add(1);
        latest.published_at_ms = published_at_ms;
        IntrospectionProbeRecord {
            recorded: true,
            dropped: false,
        }
    }

    fn force_record_bytes(&self, payload: Vec<u8>, published_at_ms: Option<u64>) {
        let mut latest = self
            .inner
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        latest.published_count = latest.published_count.saturating_add(1);
        latest.payload = Some(payload);
        latest.published_at_ms = published_at_ms;
    }

    fn snapshot(&self) -> IntrospectionChannelSnapshot {
        let latest = self
            .inner
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        IntrospectionChannelSnapshot {
            published_count: latest.published_count,
            payload: latest.payload.clone(),
            published_at_ms: latest.published_at_ms,
        }
    }
}

/// 连接作用域 observer guard。
#[derive(Debug)]
pub struct IntrospectionObserverGuard {
    inner: Arc<ChannelProbeInner>,
}

impl Drop for IntrospectionObserverGuard {
    fn drop(&mut self) {
        let mut current = self.inner.observer_count.load(Ordering::Acquire);
        while current != 0 {
            match self.inner.observer_count.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ParamState {
    ty: String,
    update: String,
    current: serde_json::Value,
    pending: Option<serde_json::Value>,
    min: Option<serde_json::Value>,
    max: Option<serde_json::Value>,
    choices: Vec<serde_json::Value>,
}

/// runtime shell 可共享更新的 introspection live 状态。
#[derive(Debug, Clone, Default)]
pub struct IntrospectionState {
    inner: Arc<Mutex<IntrospectionStateInner>>,
}

#[derive(Debug, Default)]
struct IntrospectionStateInner {
    tick_count: u64,
    self_description_json: Option<String>,
    channels: BTreeMap<String, ChannelState>,
    params: BTreeMap<String, ParamState>,
}

impl IntrospectionState {
    /// 构造空 live 状态。
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_inner(&self) -> MutexGuard<'_, IntrospectionStateInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 预注册一个 channel，使其在尚未发布样本时也出现在 status 中。
    pub fn register_channel(&self, name: impl Into<String>, message_type: impl Into<String>) {
        self.register_channel_with_probe_capacity(name, message_type, None);
    }

    /// 预注册一个带有有界 probe snapshot 容量的 channel。
    pub fn register_channel_with_probe_capacity(
        &self,
        name: impl Into<String>,
        message_type: impl Into<String>,
        max_payload_len: Option<usize>,
    ) {
        let name = name.into();
        let mut inner = self.lock_inner();
        inner.channels.entry(name).or_insert_with(|| ChannelState {
            message_type: message_type.into(),
            probe: IntrospectionChannelProbe::new(max_payload_len),
        });
    }

    /// 注册一个 runtime 参数，使 CLI 能查询并提交 pending 更新。
    pub fn register_param(&self, schema: IntrospectionParamSchema) {
        let mut inner = self.lock_inner();
        inner
            .params
            .entry(schema.name)
            .or_insert_with(|| ParamState {
                ty: schema.ty,
                update: schema.update,
                current: schema.current,
                pending: None,
                min: schema.min,
                max: schema.max,
                choices: schema.choices,
            });
    }

    /// 注册编译期 self-description JSON，供在线 CLI 自动发现和格式化。
    pub fn set_self_description_json(&self, json: impl Into<String>) {
        let mut inner = self.lock_inner();
        inner.self_description_json = Some(json.into());
    }

    /// 返回当前 runtime 暴露的 self-description JSON。
    pub fn self_description_json(&self) -> Option<String> {
        let inner = self.lock_inner();
        inner.self_description_json.clone()
    }

    /// 增加 scheduler tick 计数。
    pub fn record_tick(&self) {
        let mut inner = self.lock_inner();
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
        let mut inner = self.lock_inner();
        let channel = inner.channels.entry(name).or_insert_with(|| ChannelState {
            message_type: message_type.clone(),
            probe: IntrospectionChannelProbe::new(None),
        });
        channel.message_type = message_type;
        channel.probe.force_record_bytes(payload, published_at_ms);
    }

    /// 获取指定 channel 的 probe handle。
    pub fn channel_probe(&self, name: &str) -> Option<IntrospectionChannelProbe> {
        let inner = self.lock_inner();
        inner
            .channels
            .get(name)
            .map(|channel| channel.probe.clone())
    }

    /// 为指定 channel 建立连接作用域 observer guard。
    pub fn observe_channel(&self, name: &str) -> Option<IntrospectionObserverGuard> {
        self.channel_probe(name).map(|probe| probe.observe())
    }

    /// 返回指定 channel 当前 active observer 数量。
    pub fn active_probe_count(&self, name: &str) -> Option<u64> {
        self.channel_probe(name).map(|probe| probe.active_count())
    }

    /// 按需记录 channel 发布的 raw ABI bytes。
    pub fn try_probe_channel_publish_bytes(
        &self,
        name: &str,
        message_type: impl Into<String>,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> IntrospectionProbeRecord {
        let message_type = message_type.into();
        let probe = {
            let mut inner = self.lock_inner();
            let channel = inner
                .channels
                .entry(name.to_string())
                .or_insert_with(|| ChannelState {
                    message_type: message_type.clone(),
                    probe: IntrospectionChannelProbe::new(None),
                });
            channel.message_type = message_type;
            channel.probe.clone()
        };
        probe.try_record_bytes(payload, published_at_ms)
    }

    /// 按需记录 channel 发布的 Message ABI 对象表示。
    pub fn try_probe_channel_publish<T: Copy>(
        &self,
        name: &str,
        message_type: impl Into<String>,
        value: &T,
        published_at_ms: Option<u64>,
    ) -> IntrospectionProbeRecord {
        let Some(probe) = self.channel_probe(name) else {
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: false,
            };
        };
        if !probe.enabled() {
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: false,
            };
        }
        self.try_probe_channel_publish_bytes(
            name,
            message_type,
            bytes_of(value).as_slice(),
            published_at_ms,
        )
    }

    /// 返回当前 status 快照。
    pub fn status(&self) -> IntrospectionStatus {
        let inner = self.lock_inner();
        IntrospectionStatus {
            tick_count: inner.tick_count,
            channels: inner
                .channels
                .iter()
                .map(|(name, channel)| IntrospectionChannelStatus {
                    name: name.clone(),
                    message_type: channel.message_type.clone(),
                    published_count: channel.probe.snapshot().published_count,
                    last_payload_len: channel.probe.snapshot().payload.as_ref().map(Vec::len),
                    active_observers: channel.probe.active_count(),
                    dropped_samples: channel.probe.dropped_samples(),
                })
                .collect(),
        }
    }

    /// 返回指定 channel 的 raw ABI snapshot。
    pub fn channel_snapshot(&self, name: &str) -> Option<IntrospectionChannelSnapshot> {
        let inner = self.lock_inner();
        inner
            .channels
            .get(name)
            .map(|channel| channel.probe.snapshot())
    }

    fn channel_status(&self, name: &str) -> Option<IntrospectionChannelStatus> {
        let inner = self.lock_inner();
        inner.channels.get(name).map(|channel| {
            let snapshot = channel.probe.snapshot();
            IntrospectionChannelStatus {
                name: name.to_string(),
                message_type: channel.message_type.clone(),
                published_count: snapshot.published_count,
                last_payload_len: snapshot.payload.as_ref().map(Vec::len),
                active_observers: channel.probe.active_count(),
                dropped_samples: channel.probe.dropped_samples(),
            }
        })
    }

    /// 返回参数状态列表。
    pub fn params(&self) -> Vec<IntrospectionParamStatus> {
        let inner = self.lock_inner();
        inner
            .params
            .iter()
            .map(|(name, param)| param_status(name, param))
            .collect()
    }

    /// 返回单个参数状态。
    pub fn param(&self, name: &str) -> Option<IntrospectionParamStatus> {
        let inner = self.lock_inner();
        inner
            .params
            .get(name)
            .map(|param| param_status(name, param))
    }

    /// 设置参数 pending 值。
    pub fn set_param_pending(
        &self,
        name: &str,
        value: serde_json::Value,
    ) -> std::result::Result<IntrospectionParamStatus, String> {
        let mut inner = self.lock_inner();
        let Some(param) = inner.params.get_mut(name) else {
            return Err(format!("unknown FlowRT parameter `{name}`"));
        };
        if param.update != "on_tick" {
            return Err(format!("FlowRT parameter `{name}` is startup-only"));
        }
        validate_param_json_value(name, param, &value)?;
        param.pending = Some(value);
        Ok(param_status(name, param))
    }

    /// 读取并清空参数 pending 值，供 generated shell 在 tick 边界应用。
    pub fn take_pending_param(&self, name: &str) -> Option<serde_json::Value> {
        let mut inner = self.lock_inner();
        inner
            .params
            .get_mut(name)
            .and_then(|param| param.pending.take())
    }

    /// 查询参数 pending 值，主要用于测试和 generated shell 快速检查。
    pub fn pending_param(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.lock_inner();
        inner
            .params
            .get(name)
            .and_then(|param| param.pending.clone())
    }

    /// 记录参数已经由 generated shell 应用为当前值。
    pub fn record_param_applied(&self, name: &str, value: serde_json::Value) {
        let mut inner = self.lock_inner();
        if let Some(param) = inner.params.get_mut(name) {
            param.current = value;
            param.pending = None;
        }
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
    reclaim_stale_socket_path(&path)?;
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let server_path = path.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    let handshake = handshake.clone();
                    let state = state.clone();
                    let _ = thread::Builder::new()
                        .name("flowrt-introspection-client".to_string())
                        .spawn(move || {
                            let _ = handle_connection(stream, &handshake, &state);
                        });
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

fn reclaim_stale_socket_path(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("FlowRT runtime socket `{}` is already live", path.display()),
        )),
        Err(_) => fs::remove_file(path),
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

/// 向 introspection socket 请求 self-description JSON。
pub fn request_self_description(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::SelfDescription)
}

/// 向 introspection socket 打开 observe channel 连接。
pub fn observe_channel_stream(
    path: &Path,
    channel: impl Into<String>,
) -> std::io::Result<(UnixStream, IntrospectionResponse)> {
    let mut stream = UnixStream::connect(path)?;
    let request = serde_json::to_string(&IntrospectionRequest::ObserveChannel {
        channel: channel.into(),
        mode: Some("latest".to_string()),
    })
    .map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if line.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "FlowRT observe channel response was empty",
        ));
    }
    let response = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
    Ok((stream, response))
}

/// 向 introspection socket 请求参数列表。
pub fn request_param_list(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::ParamList)
}

/// 向 introspection socket 请求单个参数状态。
pub fn request_param_get(
    path: &Path,
    name: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::ParamGet { name: name.into() })
}

/// 向 introspection socket 写入参数 pending 值。
pub fn request_param_set(
    path: &Path,
    name: impl Into<String>,
    value: serde_json::Value,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::ParamSet {
            name: name.into(),
            value,
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
            let Some(guard) = state.observe_channel(&channel) else {
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
            let mut reader = BufReader::new(stream);
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
    }
    Ok(())
}

fn param_status(name: &str, param: &ParamState) -> IntrospectionParamStatus {
    IntrospectionParamStatus {
        name: name.to_string(),
        ty: param.ty.clone(),
        update: param.update.clone(),
        current: param.current.clone(),
        pending: param.pending.clone(),
        min: param.min.clone(),
        max: param.max.clone(),
        choices: param.choices.clone(),
    }
}

fn validate_param_json_value(
    name: &str,
    param: &ParamState,
    value: &serde_json::Value,
) -> std::result::Result<(), String> {
    if !json_value_matches_param_type(&param.ty, value) {
        return Err(format!(
            "FlowRT parameter `{name}` expects `{}` value",
            param.ty
        ));
    }
    if let Some(min) = &param.min
        && compare_json_values(value, min).is_some_and(|ordering| ordering.is_lt())
    {
        return Err(format!("FlowRT parameter `{name}` is below minimum"));
    }
    if let Some(max) = &param.max
        && compare_json_values(value, max).is_some_and(|ordering| ordering.is_gt())
    {
        return Err(format!("FlowRT parameter `{name}` is above maximum"));
    }
    if !param.choices.is_empty() && !param.choices.iter().any(|choice| choice == value) {
        return Err(format!(
            "FlowRT parameter `{name}` is not in declared enum choices"
        ));
    }
    Ok(())
}

fn json_value_matches_param_type(ty: &str, value: &serde_json::Value) -> bool {
    match ty {
        "bool" => value.is_boolean(),
        "string" => value.is_string(),
        "f32" | "f64" => value.is_number(),
        "u8" | "u16" | "u32" | "u64" => value.as_u64().is_some(),
        "i8" | "i16" | "i32" | "i64" => value.as_i64().is_some(),
        "array" => value.is_array(),
        "table" => value.is_object(),
        _ => false,
    }
}

fn compare_json_values(
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            left.as_f64()?.partial_cmp(&right.as_f64()?)
        }
        (serde_json::Value::String(left), serde_json::Value::String(right)) => {
            Some(left.cmp(right))
        }
        _ => None,
    }
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
                active_observers: 0,
                dropped_samples: 0,
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
        assert_eq!(channel.published_count, 3);
        assert_eq!(channel.payload, Some(vec![3u8; 48]));
        assert_eq!(channel.published_at_ms, Some(9));
        let channel_json = serde_json::to_value(&channel).unwrap();
        assert!(channel_json.get("name").is_none());
        assert!(channel_json.get("message_type").is_none());

        let IntrospectionResponse::Error { message, .. } =
            request_channel_snapshot(server.path(), "missing.channel")
                .expect("missing channel should return structured error response")
        else {
            panic!("missing channel request returned wrong response")
        };
        assert_eq!(message, "unknown FlowRT channel");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_returns_registered_self_description_json() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-selfdesc-test-{}",
            std::process::id()
        ));
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
        state.set_self_description_json(r#"{"package":{"name":"robot_demo"}}"#);
        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state)
            .expect("server should start");

        let IntrospectionResponse::SelfDescription {
            handshake: response_handshake,
            json,
        } = request_self_description(server.path())
            .expect("self-description request should succeed")
        else {
            panic!("self-description request returned wrong response")
        };

        assert_eq!(response_handshake, handshake);
        assert_eq!(json, r#"{"package":{"name":"robot_demo"}}"#);

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_reports_missing_self_description() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-missing-selfdesc-test-{}",
            std::process::id()
        ));
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
        let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect("server should start");

        let IntrospectionResponse::Error { message, .. } = request_self_description(server.path())
            .expect("missing self-description should return structured error")
        else {
            panic!("missing self-description request returned wrong response")
        };

        assert_eq!(message, "FlowRT self-description is not registered");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn state_recovers_after_mutex_poison() {
        let state = IntrospectionState::new();
        let poison_state = state.clone();
        let poison_thread = thread::spawn(move || {
            let _guard = poison_state.inner.lock().unwrap();
            panic!("poison introspection state for test");
        });
        assert!(poison_thread.join().is_err());

        state.register_channel("source.count_to_sink.count", "Count");
        state.record_tick();
        state.record_channel_publish_bytes("source.count_to_sink.count", "Count", vec![7], Some(1));
        state.register_param(IntrospectionParamSchema {
            name: "controller.kp".to_string(),
            ty: "f32".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(1.0),
            min: None,
            max: None,
            choices: Vec::new(),
        });

        let status = state.status();
        assert_eq!(status.tick_count, 1);
        assert_eq!(status.channels.len(), 1);
        assert_eq!(
            state
                .channel_snapshot("source.count_to_sink.count")
                .unwrap()
                .payload,
            Some(vec![7])
        );
        assert!(
            state
                .set_param_pending("controller.kp", serde_json::json!(2.0))
                .is_ok()
        );
        assert_eq!(
            state.pending_param("controller.kp"),
            Some(serde_json::json!(2.0))
        );
        assert_eq!(
            state.take_pending_param("controller.kp"),
            Some(serde_json::json!(2.0))
        );
        state.record_param_applied("controller.kp", serde_json::json!(2.0));
        assert_eq!(
            state.param("controller.kp").unwrap().current,
            serde_json::json!(2.0)
        );
    }

    #[test]
    fn probe_recording_is_disabled_until_observer_guard_is_active() {
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");

        assert!(
            !state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[1, 2, 3, 4],
                    Some(10)
                )
                .recorded
        );
        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 0);
        assert_eq!(snapshot.payload, None);

        let guard = state
            .observe_channel("source.imu_to_sink.imu")
            .expect("registered channel should be observable");
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
        assert!(
            state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[5, 6, 7, 8],
                    Some(11)
                )
                .recorded
        );
        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 1);
        assert_eq!(snapshot.payload, Some(vec![5, 6, 7, 8]));
        assert_eq!(snapshot.published_at_ms, Some(11));

        drop(guard);
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
        assert!(
            !state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[9, 10, 11, 12],
                    Some(12)
                )
                .recorded
        );
        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 1);
        assert_eq!(snapshot.payload, Some(vec![5, 6, 7, 8]));
    }

    #[test]
    fn bounded_probe_drops_oversized_payload_and_reports_drop_count() {
        let state = IntrospectionState::new();
        state.register_channel_with_probe_capacity("source.image_to_sink.image", "Image", Some(4));
        let guard = state
            .observe_channel("source.image_to_sink.image")
            .expect("registered channel should be observable");

        let record = state.try_probe_channel_publish_bytes(
            "source.image_to_sink.image",
            "Image",
            &[1, 2, 3, 4, 5],
            Some(10),
        );
        let snapshot = state
            .channel_snapshot("source.image_to_sink.image")
            .expect("registered channel should have snapshot state");
        let status = state
            .channel_status("source.image_to_sink.image")
            .expect("registered channel should have status");

        assert_eq!(
            record,
            IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            }
        );
        assert_eq!(snapshot.published_count, 0);
        assert_eq!(snapshot.payload.as_deref(), Some([].as_slice()));
        assert_eq!(status.active_observers, 1);
        assert_eq!(status.dropped_samples, 1);

        drop(guard);
        assert_eq!(
            state.active_probe_count("source.image_to_sink.image"),
            Some(0)
        );
    }

    #[test]
    fn observe_channel_socket_enables_probe_until_connection_closes() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-test-{}",
            std::process::id()
        ));
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
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let mut stream = UnixStream::connect(server.path()).unwrap();
        stream
            .write_all(
                br#"{"command":"observe_channel","channel":"source.imu_to_sink.imu","mode":"latest"}
"#,
            )
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains(r#""response":"observe_ready""#));

        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
        assert!(
            state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[1, 2, 3],
                    Some(7)
                )
                .recorded
        );
        assert_eq!(
            state
                .channel_snapshot("source.imu_to_sink.imu")
                .unwrap()
                .payload,
            Some(vec![1, 2, 3])
        );

        drop(reader);
        drop(stream);
        for _ in 0..100 {
            if state.active_probe_count("source.imu_to_sink.imu") == Some(0) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn observe_unknown_channel_returns_error_without_enabling_probe() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-missing-test-{}",
            std::process::id()
        ));
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
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let (_stream, response) = observe_channel_stream(server.path(), "missing.channel")
            .expect("missing channel should return structured error");

        assert!(matches!(
            response,
            IntrospectionResponse::Error { message, .. } if message == "unknown FlowRT channel"
        ));
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
        assert_eq!(state.active_probe_count("missing.channel"), None);

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn observe_channel_stream_helper_keeps_probe_enabled_until_stream_drops() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-helper-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 43,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let (stream, response) =
            observe_channel_stream(server.path(), "source.imu_to_sink.imu").unwrap();
        assert!(matches!(
            response,
            IntrospectionResponse::ObserveReady { .. }
        ));
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
        assert!(
            state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[9, 8, 7],
                    Some(8)
                )
                .recorded
        );

        drop(stream);
        for _ in 0..100 {
            if state.active_probe_count("source.imu_to_sink.imu") == Some(0) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_refuses_to_replace_live_socket() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-live-socket-test-{}",
            std::process::id()
        ));
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
        let first =
            spawn_status_server_at(socket.clone(), handshake.clone(), IntrospectionState::new())
                .expect("first server should start");

        let error = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect_err("live socket must not be replaced by a second server");

        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        assert!(request_status(first.path()).is_ok());

        drop(first);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_removes_stale_socket_file_before_binding() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-stale-socket-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        fs::create_dir_all(&root).unwrap();
        fs::write(&socket, b"stale").unwrap();
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };

        let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect("stale socket path should be reclaimed");

        assert!(request_status(server.path()).is_ok());
        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_handles_runtime_parameter_requests() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-params-test-{}",
            std::process::id()
        ));
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
        state.register_param(IntrospectionParamSchema {
            name: "controller.kp".to_string(),
            ty: "f32".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(1.0),
            min: Some(serde_json::json!(0.0)),
            max: Some(serde_json::json!(10.0)),
            choices: Vec::new(),
        });
        state.register_param(IntrospectionParamSchema {
            name: "controller.mode".to_string(),
            ty: "string".to_string(),
            update: "startup".to_string(),
            current: serde_json::json!("normal"),
            min: None,
            max: None,
            choices: vec![serde_json::json!("normal"), serde_json::json!("safe")],
        });

        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
            .expect("server should start");

        let IntrospectionResponse::ParamList { params, .. } =
            request_param_list(server.path()).expect("param list request should succeed")
        else {
            panic!("param list returned wrong response")
        };
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "controller.kp");
        assert_eq!(params[0].current, serde_json::json!(1.0));
        assert!(params[0].pending.is_none());

        let IntrospectionResponse::ParamValue { param, .. } =
            request_param_get(server.path(), "controller.kp")
                .expect("param get request should succeed")
        else {
            panic!("param get returned wrong response")
        };
        assert_eq!(param.current, serde_json::json!(1.0));

        let IntrospectionResponse::ParamValue { param, .. } =
            request_param_set(server.path(), "controller.kp", serde_json::json!(2.5))
                .expect("param set request should succeed")
        else {
            panic!("param set returned wrong response")
        };
        assert_eq!(param.current, serde_json::json!(1.0));
        assert_eq!(param.pending, Some(serde_json::json!(2.5)));
        assert_eq!(
            state.pending_param("controller.kp"),
            Some(serde_json::json!(2.5))
        );
        state.record_param_applied("controller.kp", serde_json::json!(2.5));
        assert_eq!(state.pending_param("controller.kp"), None);

        let IntrospectionResponse::Error { message, .. } =
            request_param_set(server.path(), "controller.mode", serde_json::json!("safe"))
                .expect("startup param set should return structured error")
        else {
            panic!("startup param set returned wrong response")
        };
        assert_eq!(
            message,
            "FlowRT parameter `controller.mode` is startup-only"
        );

        let IntrospectionResponse::Error { message, .. } =
            request_param_set(server.path(), "controller.kp", serde_json::json!(12.0))
                .expect("out-of-range param set should return structured error")
        else {
            panic!("out-of-range param set returned wrong response")
        };
        assert_eq!(message, "FlowRT parameter `controller.kp` is above maximum");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }
}
