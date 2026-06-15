//! Zenoh supervisor 自动 mesh 启动环境。

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;

use super::manifest::LaunchProcess;

/// 本机 zenoh 自动 mesh 使用 IANA dynamic/private port range。
const ZENOH_AUTO_PORT_BASE: u16 = 49_152;
const ZENOH_AUTO_PORT_COUNT: u16 = 16_384;

/// Zenoh 启动环境变量。
#[derive(Debug, Clone)]
pub struct ZenohLaunchEnv {
    pub listen: String,
    pub connect: String,
}

/// Zenoh 启动计划。
///
/// `port_lease` 必须和该 graph 的子进程生命周期一起持有，避免同一台机器上多个
/// FlowRT supervisor 自动选择同一个本机 zenoh 端口。
#[derive(Debug)]
pub struct ZenohLaunchPlan {
    pub env: HashMap<String, ZenohLaunchEnv>,
    _port_lease: Option<ZenohPortLease>,
}

impl ZenohLaunchPlan {
    pub(super) fn empty() -> Self {
        Self {
            env: HashMap::new(),
            _port_lease: None,
        }
    }
}

#[derive(Debug)]
pub(super) struct ZenohPortLease {
    pub(super) port: u16,
    file: File,
}

impl Drop for ZenohPortLease {
    fn drop(&mut self) {
        let _ = unlock_zenoh_port_file(&self.file);
    }
}

/// 检查是否需要自动配置 zenoh（用户未显式设置相关环境变量时）。
pub fn should_auto_configure_zenoh() -> bool {
    std::env::var_os("FLOWRT_ZENOH_MODE").is_none()
        && std::env::var_os("FLOWRT_ZENOH_LISTEN").is_none()
        && std::env::var_os("FLOWRT_ZENOH_CONNECT").is_none()
}

/// 为 graph 中的 zenoh 进程生成 hub-and-spoke 拓扑配置。
///
/// 第一个 zenoh backend 进程作为 hub，监听 FlowRT supervisor 持有租约的本机端口；
/// 其余进程连接该 hub。
pub fn zenoh_launch_env_for_graph(processes: &[&LaunchProcess]) -> Result<ZenohLaunchPlan, String> {
    let zenoh_processes: Vec<&LaunchProcess> = processes
        .iter()
        .filter(|p| p.backend == "zenoh")
        .copied()
        .collect();
    if zenoh_processes.is_empty() {
        return Ok(ZenohLaunchPlan::empty());
    }

    let hub = zenoh_processes[0];
    let port_lease = reserve_zenoh_port_lease(&hub.name)?;
    let hub_locator = format!("tcp/127.0.0.1:{}", port_lease.port);

    let mut env = HashMap::new();
    for process in zenoh_processes {
        let listen = if process.name == hub.name {
            hub_locator.clone()
        } else {
            String::new()
        };
        let connect = if process.name == hub.name {
            String::new()
        } else {
            hub_locator.clone()
        };
        env.insert(process.name.clone(), ZenohLaunchEnv { listen, connect });
    }
    Ok(ZenohLaunchPlan {
        env,
        _port_lease: Some(port_lease),
    })
}

pub(super) fn reserve_zenoh_port_lease(process_name: &str) -> Result<ZenohPortLease, String> {
    let seed = std::process::id() as u16;
    for offset in 0..ZENOH_AUTO_PORT_COUNT {
        let port = ZENOH_AUTO_PORT_BASE + seed.wrapping_add(offset) % ZENOH_AUTO_PORT_COUNT;
        match try_acquire_zenoh_port_lease(port) {
            Ok(Some(lease)) => return Ok(lease),
            Ok(None) => continue,
            Err(error) => {
                return Err(format!(
                    "failed to reserve local zenoh port lease for `{process_name}`: {error}"
                ));
            }
        }
    }
    Err(format!(
        "failed to reserve local zenoh port lease for `{process_name}`: no FlowRT auto ports are available"
    ))
}

fn try_acquire_zenoh_port_lease(port: u16) -> std::io::Result<Option<ZenohPortLease>> {
    let path = std::env::temp_dir().join(format!("flowrt.zenoh.{port}.lock"));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if !try_lock_zenoh_port_file(&file)? {
        return Ok(None);
    }
    file.set_len(0)?;
    writeln!(file, "pid={}", std::process::id())?;
    Ok(Some(ZenohPortLease { port, file }))
}

#[cfg(unix)]
fn try_lock_zenoh_port_file(file: &File) -> std::io::Result<bool> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(error),
    }
}

#[cfg(unix)]
fn unlock_zenoh_port_file(file: &File) -> std::io::Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
