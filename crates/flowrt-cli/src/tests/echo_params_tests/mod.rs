use super::*;
use crate::introspection::{EchoFormatOptions, select_echo_socket};

static XDG_RUNTIME_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvOverride {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvOverride {
    fn set(key: &'static str, value: Option<&std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: tests guard process-wide environment mutation with a mutex.
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { key, previous }
    }
}

impl Drop for EnvOverride {
    fn drop(&mut self) {
        // SAFETY: guarded by XDG_RUNTIME_DIR_ENV_LOCK in tests below.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
use zenoh::Wait;

mod echo;
mod live_params;
mod pub_cmd;
mod remote_params;
mod replay;
