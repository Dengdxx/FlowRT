//! FlowRT Rust runtime 的基础 API。
//!
//! 本 crate 只承载 runtime shell 和用户组件需要直接接触的薄接口：状态、上下文、输入视图、
//! 输出句柄、channel 语义和 backend 抽象。用户算法不应依赖具体通信库 API。

pub mod backend;
pub mod channel;
pub mod inproc;
pub mod introspection;
#[cfg(feature = "iox2")]
pub mod iox2;
pub mod wire;

pub use backend::{
    Backend, BackendKind, InprocBackend, InprocScheduler, Iox2Backend, Scheduler, ZenohBackend,
    inproc_backend, iox2_backend, zenoh_backend,
};
pub use channel::{
    BackendCapabilities, ChannelError, ChannelWriteOutcome, FifoChannel, FifoRead, LatestChannel,
    OverflowPolicy, StaleConfig, StalePolicy,
};
#[cfg(feature = "iox2")]
pub use iceoryx2::prelude::ZeroCopySend;
pub use introspection::{
    INTROSPECTION_PROTOCOL_VERSION, IntrospectionChannelStatus, IntrospectionHandshake,
    IntrospectionIdentity, IntrospectionRequest, IntrospectionResponse, IntrospectionServer,
    IntrospectionState, IntrospectionStatus, discover_runtime_sockets, request_channel_snapshot,
    request_status, runtime_socket_dir, runtime_socket_path_for_pid, spawn_status_server,
    spawn_status_server_at,
};
pub use wire::{WireCodec, WireCodecError};

/// 生成组件接口返回的执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// 本次步骤完成，调度器可以继续执行后续 tick。
    #[default]
    Ok,
    /// 本次步骤未完成，调用方可按调度策略稍后重试。
    Retry,
    /// 本次步骤失败，调度器应停止当前运行序列并向上报告。
    Error,
}

impl Status {
    /// 返回成功状态。
    pub const fn ok() -> Self {
        Self::Ok
    }
}

/// runtime 传递给生命周期钩子和调度步骤的上下文。
///
/// v0.1 暂不暴露资源句柄；保留该类型是为了后续承载 clock、logger、参数快照和 backend
/// 能力视图，同时保持用户接口稳定。
#[derive(Debug, Default)]
pub struct Context {
    _private: (),
}

/// latest snapshot 输入视图。
///
/// `Latest<'_, T>` 不拥有消息对象，只在一次用户回调期间借用 runtime shell 中的最新样本。
/// `present()` 表示当前是否有可读样本，`stale()` 表示样本是否超过 RSDL 声明的 freshness
/// 约束。用户代码不得保存内部引用到回调之外。
#[derive(Debug, Clone, Copy)]
pub struct Latest<'a, T> {
    value: Option<&'a T>,
    stale: bool,
}

impl<'a, T> Latest<'a, T> {
    /// 从可选借用值和 stale 标记构造输入视图。
    pub fn new(value: Option<&'a T>, stale: bool) -> Self {
        Self { value, stale }
    }

    /// 判断当前输入是否有样本。
    pub fn present(&self) -> bool {
        self.value.is_some()
    }

    /// 判断当前样本是否已过期。
    pub fn stale(&self) -> bool {
        self.stale
    }

    /// 借用当前样本。
    pub fn as_ref(&self) -> Option<&'a T> {
        self.value
    }
}

/// 组件输出端口的单样本写入句柄。
///
/// 用户回调通过 `write()` 设置本次输出。runtime shell 在回调返回后取走该值并发布到对应
/// channel；如果用户没有写入，则该端口本次 tick 不产生输出。
#[derive(Debug, Default)]
pub struct Output<T> {
    value: Option<T>,
}

impl<T> Output<T> {
    /// 构造空输出句柄。
    pub fn new() -> Self {
        Self { value: None }
    }

    /// 写入本次回调的输出样本；重复调用时后一次写入覆盖前一次值。
    pub fn write(&mut self, value: T) {
        self.value = Some(value);
    }

    /// 取走当前输出样本并清空句柄。
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }

    /// 借用当前输出样本。
    pub fn as_ref(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// 可变借用当前输出样本。
    pub fn as_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_tracks_presence_and_staleness() {
        let value = 42u32;
        let latest = Latest::new(Some(&value), true);
        assert!(latest.present());
        assert!(latest.stale());
        assert_eq!(latest.as_ref(), Some(&42));
    }

    #[test]
    fn output_can_store_and_take_values() {
        let mut output = Output::new();
        output.write(7u32);
        assert_eq!(output.as_ref(), Some(&7));
        assert_eq!(output.take(), Some(7));
        assert!(output.as_ref().is_none());
    }
}
