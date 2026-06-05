//! FlowRT runtime 的进程关闭信号。
//!
//! 生成的 runtime shell 使用 `ShutdownToken` 把 Unix 信号转成调度循环可观察的状态，确保
//! `shutdown` task、`on_stop` 和 `on_shutdown` 在可恢复退出路径上执行。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

static SIGNAL_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static SIGNAL_HANDLERS_INSTALLED: AtomicBool = AtomicBool::new(false);

/// runtime 调度循环共享的关闭请求。
#[derive(Debug, Clone, Default)]
pub struct ShutdownToken {
    requested: Arc<AtomicBool>,
    external: Option<&'static AtomicBool>,
}

impl ShutdownToken {
    /// 构造未触发的关闭 token。
    pub fn new() -> Self {
        Self::default()
    }

    /// 构造测试用 token。
    pub fn new_for_test() -> Self {
        Self::new()
    }

    fn with_external(external: &'static AtomicBool) -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
            external: Some(external),
        }
    }

    /// 标记需要优雅退出。
    pub fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    /// 判断是否已有关闭请求。
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
            || self
                .external
                .is_some_and(|external| external.load(Ordering::SeqCst))
    }
}

/// 安装 SIGINT/SIGTERM handler，并返回可被调度循环查询的 token。
///
/// handler 只设置 atomic flag，避免在异步信号上下文执行非安全逻辑。多次调用会复用同一个
/// 进程级信号 flag；每次调用返回新的 token，并在调用时同步当前信号状态。
pub fn install_signal_shutdown_token() -> ShutdownToken {
    install_signal_handlers_once();
    ShutdownToken::with_external(&SIGNAL_SHUTDOWN_REQUESTED)
}

fn install_signal_handlers_once() {
    if SIGNAL_HANDLERS_INSTALLED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    unsafe {
        let handler = handle_signal as *const () as libc::sighandler_t;
        let _ = libc::signal(libc::SIGINT, handler);
        let _ = libc::signal(libc::SIGTERM, handler);
    }
}

extern "C" fn handle_signal(_signal: libc::c_int) {
    SIGNAL_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_token_tracks_manual_request() {
        let token = ShutdownToken::new_for_test();

        assert!(!token.is_requested());
        token.request();
        assert!(token.is_requested());
    }

    #[test]
    fn signal_shutdown_token_observes_signal_handler_flag() {
        SIGNAL_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        let token = install_signal_shutdown_token();

        assert!(!token.is_requested());
        handle_signal(libc::SIGTERM);
        assert!(token.is_requested());
        SIGNAL_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }
}
