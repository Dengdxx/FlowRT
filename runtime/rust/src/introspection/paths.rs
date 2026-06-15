use std::fs;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
pub(super) fn reclaim_stale_socket_path(path: &Path) -> std::io::Result<()> {
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
pub(super) fn unix_time_ms() -> u64 {
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
