//! FlowRT runtime 自描述与 live status 的最小 Unix socket 协议。
//!
//! socket 路径只用于发现候选进程；真实身份必须来自连接后的 handshake。协议保持同步、
//! JSON-line 和标准库实现，便于生成 shell 在不引入大型 runtime 依赖的情况下接入。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 当前 introspection 协议版本。
pub const INTROSPECTION_PROTOCOL_VERSION: &str = "0.1";

/// runtime introspection 命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum IntrospectionRequest {
    /// 返回进程 handshake 与当前 live status。
    Status,
}

/// runtime introspection 响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionResponse {
    pub handshake: IntrospectionHandshake,
    pub status: IntrospectionStatus,
}

/// CLI 连接 socket 后首先验证的进程身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionHandshake {
    pub protocol_version: String,
    pub pid: u32,
    pub started_at_unix_ms: u128,
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
    status: IntrospectionStatus,
) -> std::io::Result<IntrospectionServer> {
    let handshake = identity.handshake();
    let path = runtime_socket_path_for_pid(handshake.pid);
    spawn_status_server_at(path, handshake, status)
}

/// 在指定路径启动一个最小 introspection status 服务，主要用于测试。
pub fn spawn_status_server_at(
    path: PathBuf,
    handshake: IntrospectionHandshake,
    status: IntrospectionStatus,
) -> std::io::Result<IntrospectionServer> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    let server_path = path.clone();
    let handle = thread::spawn(move || {
        if let Ok((stream, _addr)) = listener.accept() {
            let _ = handle_connection(stream, handshake, status);
        }
    });
    Ok(IntrospectionServer {
        path: server_path,
        handle: Some(handle),
    })
}

/// 已启动的 introspection 服务。
#[derive(Debug)]
pub struct IntrospectionServer {
    path: PathBuf,
    handle: Option<JoinHandle<()>>,
}

impl IntrospectionServer {
    /// 返回服务 socket 路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for IntrospectionServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(handle) = self.handle.take() {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

/// 向 introspection socket 请求 status。
pub fn request_status(path: &Path) -> std::io::Result<IntrospectionResponse> {
    let mut stream = UnixStream::connect(path)?;
    let request =
        serde_json::to_string(&IntrospectionRequest::Status).map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

fn handle_connection(
    stream: UnixStream,
    handshake: IntrospectionHandshake,
    status: IntrospectionStatus,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request =
        serde_json::from_str::<IntrospectionRequest>(&line).map_err(std::io::Error::other)?;
    match request {
        IntrospectionRequest::Status => {
            let response = IntrospectionResponse { handshake, status };
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

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
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
        let status = IntrospectionStatus {
            tick_count: 7,
            channels: vec![IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 3,
                last_payload_len: Some(48),
            }],
        };

        let server = spawn_status_server_at(socket.clone(), handshake.clone(), status.clone())
            .expect("server should start");
        let response = request_status(server.path()).expect("status request should succeed");

        assert_eq!(response.handshake, handshake);
        assert_eq!(response.status, status);

        drop(server);
        let _ = fs::remove_dir_all(root);
    }
}
