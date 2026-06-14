use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use crate::Latest;

/// runtime 层使用的 backend capability 列表。
///
/// capability 字符串来自 Contract IR/backend contract，例如 `channel:latest` 或
/// `topology:multi_process`。validator 使用同一套 capability 语义判断部署是否可满足。
#[derive(Debug, Clone, Copy)]
pub struct BackendCapabilities {
    capabilities: &'static [&'static str],
}

impl BackendCapabilities {
    /// 从静态 capability 切片构造视图。
    pub const fn new(capabilities: &'static [&'static str]) -> Self {
        Self { capabilities }
    }

    /// 查询 backend 是否声明某项能力。
    pub fn contains(&self, capability: &str) -> bool {
        self.capabilities.contains(&capability)
    }

    /// 返回完整 capability 列表。
    pub fn as_slice(&self) -> &'static [&'static str] {
        self.capabilities
    }
}

/// 有界 channel 写满时的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// 丢弃最旧样本，接收新样本。
    DropOldest,
    /// 丢弃当前写入样本，保留已有队列。
    DropNewest,
    /// 返回溢出错误，由 runtime shell 或用户代码处理。
    Error,
    /// 表达背压意图；实时路径不应默认使用无界阻塞。
    Block,
}

/// 输入样本过期时的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalePolicy {
    /// 保留样本并暴露 stale 标记。
    Warn,
    /// 过期后隐藏样本。
    Drop,
    /// 保留最后一个样本并暴露 stale 标记。
    HoldLast,
    /// 由 generated shell 将过期输入提升为错误状态。
    Error,
}

/// 带时间戳 channel 读取时的 freshness 配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleConfig {
    max_age_ms: Option<u64>,
    policy: StalePolicy,
}

impl StaleConfig {
    /// 构造 freshness 配置。
    pub const fn new(max_age_ms: Option<u64>, policy: StalePolicy) -> Self {
        Self { max_age_ms, policy }
    }

    /// 构造不检查过期时间的默认配置。
    pub const fn none() -> Self {
        Self {
            max_age_ms: None,
            policy: StalePolicy::Warn,
        }
    }

    /// 返回最大允许样本年龄，单位为毫秒。
    pub fn max_age_ms(&self) -> Option<u64> {
        self.max_age_ms
    }

    /// 返回样本过期时的处理策略。
    pub fn policy(&self) -> StalePolicy {
        self.policy
    }

    pub(crate) fn stale_at(&self, published_at_ms: Option<u64>, now_ms: u64) -> bool {
        match (self.max_age_ms, published_at_ms) {
            (Some(max_age), Some(published_at)) => now_ms.saturating_sub(published_at) > max_age,
            _ => false,
        }
    }
}

impl Default for StaleConfig {
    fn default() -> Self {
        Self::none()
    }
}

/// 有界 FIFO channel 写入成功后的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelWriteOutcome {
    /// 样本已进入 channel。
    Accepted,
    /// 为接收新样本丢弃了最旧样本。
    DroppedOldest,
    /// 当前样本被丢弃。
    DroppedNewest,
    /// 写入方遇到背压，样本未进入 channel。
    Backpressured,
}

/// channel 严格写入失败时的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// 有界队列已满且策略要求报告错误。
    Overflow,
}

/// latest channel 的最小内存态实现。
///
/// 该类型服务于 Rust inproc demo 和生成 shell 的语义验证。真实跨进程 backend 需要保持同样的
/// `Latest<'_, T>` 用户视图语义，但可以使用不同存储和传输机制。
#[derive(Debug, Clone)]
pub struct LatestChannel<T> {
    value: Option<T>,
    stale: bool,
    published_at_ms: Option<u64>,
    stale_config: StaleConfig,
    revision: u64,
}

impl<T> Default for LatestChannel<T> {
    fn default() -> Self {
        Self {
            value: None,
            stale: false,
            published_at_ms: None,
            stale_config: StaleConfig::default(),
            revision: 0,
        }
    }
}

impl<T> LatestChannel<T> {
    /// 构造空 latest channel。
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用 freshness 配置构造空 latest channel。
    pub fn with_stale_config(stale_config: StaleConfig) -> Self {
        Self {
            stale_config,
            ..Self::default()
        }
    }

    /// 发布一个新样本并清除 stale 标记。
    pub fn publish(&mut self, value: T) {
        self.value = Some(value);
        self.stale = false;
        self.published_at_ms = None;
        self.revision = self.revision.saturating_add(1);
    }

    /// 带 runtime 时间戳发布一个新样本。
    pub fn publish_at(&mut self, value: T, now_ms: u64) {
        self.value = Some(value);
        self.stale = false;
        self.published_at_ms = Some(now_ms);
        self.revision = self.revision.saturating_add(1);
    }

    /// 返回已进入 channel 的样本修订号，用于调度器检测新到达数据。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 设置当前样本的 stale 标记。
    pub fn mark_stale(&mut self, stale: bool) {
        self.stale = stale;
    }

    /// 借用当前 latest snapshot，不重新计算时间戳 freshness。
    pub fn view(&self) -> Latest<'_, T> {
        Latest::new(self.value.as_ref(), self.stale)
    }

    /// 以指定 runtime 时间读取 latest snapshot，并按 freshness 配置计算 stale 状态。
    pub fn view_at(&self, now_ms: u64) -> Latest<'_, T> {
        let stale = self.stale || self.stale_config.stale_at(self.published_at_ms, now_ms);
        let value = if stale && self.stale_config.policy == StalePolicy::Drop {
            None
        } else {
            self.value.as_ref()
        };
        Latest::new(value, stale)
    }

    /// 取走当前样本并清空 channel。
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }
}

#[derive(Debug, Clone)]
struct FifoEntry<T> {
    value: T,
    published_at_ms: Option<u64>,
}

/// FIFO channel 的单次读取结果。
///
/// 该类型拥有从 FIFO 队列取出的样本，并在一次调度步骤内借出 `Latest<'_, T>` 用户视图。
#[derive(Debug, Clone)]
pub struct FifoRead<T> {
    value: Option<T>,
    stale: bool,
}

impl<T> FifoRead<T> {
    fn new(value: Option<T>, stale: bool) -> Self {
        Self { value, stale }
    }

    fn empty() -> Self {
        Self::new(None, false)
    }

    /// 借用本次读取结果，形成组件输入使用的 latest-style 视图。
    pub fn view(&self) -> Latest<'_, T> {
        Latest::new(self.value.as_ref(), self.stale)
    }

    /// 取走读取结果中的样本。
    pub fn into_inner(self) -> Option<T> {
        self.value
    }
}

/// 有界 FIFO channel 的最小内存态实现。
///
/// `FifoChannel` 用于表达 RSDL 中 `fifo(depth = N)` 的基础行为。它不提供线程同步；多线程或
/// 跨进程 backend 应在自己的实现中保证并发安全，并保持相同的 overflow 语义。
#[derive(Debug, Clone)]
pub struct FifoChannel<T> {
    queue: VecDeque<FifoEntry<T>>,
    depth: usize,
    overflow: OverflowPolicy,
    stale_config: StaleConfig,
    revision: u64,
}

impl<T> FifoChannel<T> {
    /// 构造有界 FIFO channel；`depth` 为 0 时按 1 处理。
    pub fn new(depth: usize, overflow: OverflowPolicy) -> Self {
        Self {
            queue: VecDeque::with_capacity(depth.max(1)),
            depth: depth.max(1),
            overflow,
            stale_config: StaleConfig::default(),
            revision: 0,
        }
    }

    /// 使用 freshness 配置构造有界 FIFO channel。
    pub fn with_stale_config(
        depth: usize,
        overflow: OverflowPolicy,
        stale_config: StaleConfig,
    ) -> Self {
        Self {
            stale_config,
            ..Self::new(depth, overflow)
        }
    }

    /// 写入一个样本。
    pub fn push(&mut self, value: T) -> Result<ChannelWriteOutcome, ChannelError> {
        self.push_entry(FifoEntry {
            value,
            published_at_ms: None,
        })
    }

    /// 带 runtime 时间戳写入一个样本。
    pub fn push_at(&mut self, value: T, now_ms: u64) -> Result<ChannelWriteOutcome, ChannelError> {
        self.push_entry(FifoEntry {
            value,
            published_at_ms: Some(now_ms),
        })
    }

    fn push_entry(&mut self, entry: FifoEntry<T>) -> Result<ChannelWriteOutcome, ChannelError> {
        if self.queue.len() < self.depth {
            self.queue.push_back(entry);
            self.revision = self.revision.saturating_add(1);
            return Ok(ChannelWriteOutcome::Accepted);
        }

        match self.overflow {
            OverflowPolicy::DropOldest => {
                self.queue.pop_front();
                self.queue.push_back(entry);
                self.revision = self.revision.saturating_add(1);
                Ok(ChannelWriteOutcome::DroppedOldest)
            }
            OverflowPolicy::DropNewest => Ok(ChannelWriteOutcome::DroppedNewest),
            OverflowPolicy::Error => Err(ChannelError::Overflow),
            OverflowPolicy::Block => Ok(ChannelWriteOutcome::Backpressured),
        }
    }

    /// 弹出最旧样本。
    pub fn pop(&mut self) -> Option<T> {
        self.queue.pop_front().map(|entry| entry.value)
    }

    /// 以指定 runtime 时间弹出最旧样本，并按 freshness 配置计算 stale 状态。
    pub fn pop_at(&mut self, now_ms: u64) -> FifoRead<T> {
        let Some(entry) = self.queue.pop_front() else {
            return FifoRead::empty();
        };
        let stale = self.stale_config.stale_at(entry.published_at_ms, now_ms);
        let value = if stale && self.stale_config.policy == StalePolicy::Drop {
            None
        } else {
            Some(entry.value)
        };
        FifoRead::new(value, stale)
    }

    /// 返回当前队列长度。
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// 判断队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 返回归一化后的队列深度。
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// 返回已进入 channel 的样本修订号，用于调度器检测新到达数据。
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug)]
struct BoundaryInputInner<T> {
    value: Option<Arc<T>>,
    published_at_ms: Option<u64>,
    stale_config: StaleConfig,
    revision: u64,
    schedule_waiter: Option<crate::ScheduleWaiter>,
}

/// island boundary input 的显式注入端。
///
/// 该类型只服务于 `boundary.input`：测试工具、ROS2 adapter 或其他边界驱动通过
/// `inject*` 写入 latest snapshot，generated shell 通过 `read*` 读取该 snapshot。普通
/// dataflow channel 不依赖该类型，因此未启用 boundary 时不会在热路径增加 sink 或分支。
#[derive(Debug, Clone)]
pub struct BoundaryInput<T> {
    inner: Arc<Mutex<BoundaryInputInner<T>>>,
}

impl<T> Default for BoundaryInput<T> {
    fn default() -> Self {
        Self::with_stale_config(StaleConfig::default())
    }
}

impl<T> BoundaryInput<T> {
    /// 构造空 boundary input。
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用 freshness 配置构造空 boundary input。
    pub fn with_stale_config(stale_config: StaleConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BoundaryInputInner {
                value: None,
                published_at_ms: None,
                stale_config,
                revision: 0,
                schedule_waiter: None,
            })),
        }
    }

    /// 绑定 scheduler waiter；后续注入会唤醒 `on_message` / `any_ready` 等数据触发任务。
    pub fn set_schedule_waiter(&self, waiter: crate::ScheduleWaiter) {
        lock_recover(&self.inner).schedule_waiter = Some(waiter);
    }

    /// 注入一个无 runtime 时间戳的样本，返回注入后的修订号。
    pub fn inject(&self, value: T) -> u64 {
        self.inject_entry(value, None)
    }

    /// 注入一个带 runtime 毫秒时间戳的样本，返回注入后的修订号。
    pub fn inject_at(&self, value: T, now_ms: u64) -> u64 {
        self.inject_entry(value, Some(now_ms))
    }

    fn inject_entry(&self, value: T, published_at_ms: Option<u64>) -> u64 {
        let (revision, waiter) = {
            let mut inner = lock_recover(&self.inner);
            inner.value = Some(Arc::new(value));
            inner.published_at_ms = published_at_ms;
            inner.revision = inner.revision.saturating_add(1);
            (inner.revision, inner.schedule_waiter.clone())
        };
        if let Some(waiter) = waiter {
            match published_at_ms {
                Some(published_at_ms) => waiter.notify_data_at_ms(published_at_ms),
                None => waiter.notify_data(),
            }
        }
        revision
    }

    /// 借出当前 latest snapshot；不重新计算 freshness。
    pub fn read(&self) -> BoundaryInputRead<T> {
        let inner = lock_recover(&self.inner);
        BoundaryInputRead::new(inner.value.clone(), false, inner.revision)
    }

    /// 按 runtime 毫秒时间读取 latest snapshot，并应用 freshness 配置。
    pub fn read_at(&self, now_ms: u64) -> BoundaryInputRead<T> {
        let inner = lock_recover(&self.inner);
        let stale = inner.stale_config.stale_at(inner.published_at_ms, now_ms);
        let value = if stale && inner.stale_config.policy == StalePolicy::Drop {
            None
        } else {
            inner.value.clone()
        };
        BoundaryInputRead::new(value, stale, inner.revision)
    }

    /// 返回已注入样本修订号。
    pub fn revision(&self) -> u64 {
        lock_recover(&self.inner).revision
    }
}

/// boundary input 的一次读取结果。
///
/// 读取结果持有一个 `Arc` snapshot，使调用方释放锁后仍能安全构造 `Latest<'_, T>`。这条
/// 路径只存在于显式 boundary 注入，不影响普通 inproc/iox2/zenoh channel 的零额外观测路径。
#[derive(Debug, Clone)]
pub struct BoundaryInputRead<T> {
    value: Option<Arc<T>>,
    stale: bool,
    revision: u64,
}

impl<T> BoundaryInputRead<T> {
    fn new(value: Option<Arc<T>>, stale: bool, revision: u64) -> Self {
        Self {
            value,
            stale,
            revision,
        }
    }

    /// 借用本次读取结果，形成组件输入使用的 latest-style 视图。
    pub fn view(&self) -> Latest<'_, T> {
        Latest::new(self.value.as_deref(), self.stale)
    }

    /// 返回本次读取对应的注入修订号。
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

type BoundarySink<T> = Arc<dyn Fn(&T, Option<u64>) + Send + Sync + 'static>;

struct BoundaryOutputInner<T> {
    sinks: BTreeMap<u64, BoundarySink<T>>,
    next_sink_id: u64,
}

/// island boundary output 的显式观测 sink。
///
/// generated shell 只会在声明了 `boundary.output` 的端口发布到该类型。sink 由工具或 adapter
/// 显式注册，guard drop 后自动移除，避免临时观测永久改变运行时行为。
#[derive(Clone)]
pub struct BoundaryOutput<T> {
    inner: Arc<Mutex<BoundaryOutputInner<T>>>,
}

impl<T> Default for BoundaryOutput<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BoundaryOutputInner {
                sinks: BTreeMap::new(),
                next_sink_id: 0,
            })),
        }
    }
}

impl<T> BoundaryOutput<T> {
    /// 构造没有 sink 的 boundary output。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个 sink，并返回自动注销 guard。
    pub fn register_sink<F>(&self, sink: F) -> BoundaryOutputSinkGuard<T>
    where
        F: Fn(&T, Option<u64>) + Send + Sync + 'static,
    {
        let mut inner = lock_recover(&self.inner);
        let id = inner.next_sink_id;
        inner.next_sink_id = inner.next_sink_id.saturating_add(1);
        inner.sinks.insert(id, Arc::new(sink));
        BoundaryOutputSinkGuard {
            inner: Arc::downgrade(&self.inner),
            id,
        }
    }

    /// 发布一个无 runtime 时间戳的样本到当前已注册 sink。
    pub fn publish(&self, value: &T) {
        self.publish_entry(value, None);
    }

    /// 发布一个带 runtime 毫秒时间戳的样本到当前已注册 sink。
    pub fn publish_at(&self, value: &T, now_ms: u64) {
        self.publish_entry(value, Some(now_ms));
    }

    fn publish_entry(&self, value: &T, published_at_ms: Option<u64>) {
        let sinks: Vec<BoundarySink<T>> =
            lock_recover(&self.inner).sinks.values().cloned().collect();
        for sink in sinks {
            sink(value, published_at_ms);
        }
    }

    /// 返回当前注册 sink 数量，供测试和诊断使用。
    pub fn sink_count(&self) -> usize {
        lock_recover(&self.inner).sinks.len()
    }
}

/// boundary output sink 注册生命周期 guard。
pub struct BoundaryOutputSinkGuard<T> {
    inner: Weak<Mutex<BoundaryOutputInner<T>>>,
    id: u64,
}

impl<T> Drop for BoundaryOutputSinkGuard<T> {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        lock_recover(&inner).sinks.remove(&self.id);
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_channel_tracks_latest_value() {
        let mut channel = LatestChannel::new();
        channel.publish(7u32);
        assert_eq!(channel.view().as_ref(), Some(&7));
        assert_eq!(channel.take(), Some(7));
    }

    #[test]
    fn fifo_channel_honors_overflow_policy() {
        let mut channel = FifoChannel::new(1, OverflowPolicy::DropOldest);
        assert_eq!(channel.push(1u32), Ok(ChannelWriteOutcome::Accepted));
        assert_eq!(channel.push(2u32), Ok(ChannelWriteOutcome::DroppedOldest));
        assert_eq!(channel.pop(), Some(2));
    }

    #[test]
    fn latest_channel_marks_values_stale_after_max_age() {
        let mut channel =
            LatestChannel::with_stale_config(StaleConfig::new(Some(10), StalePolicy::Warn));
        channel.publish_at(7u32, 100);

        let fresh = channel.view_at(109);
        assert!(fresh.present());
        assert!(!fresh.stale());

        let stale = channel.view_at(111);
        assert!(stale.present());
        assert!(stale.stale());
    }

    #[test]
    fn latest_channel_drop_policy_hides_stale_values() {
        let mut channel =
            LatestChannel::with_stale_config(StaleConfig::new(Some(10), StalePolicy::Drop));
        channel.publish_at(7u32, 100);

        let stale = channel.view_at(111);
        assert!(!stale.present());
        assert!(stale.stale());
    }

    #[test]
    fn latest_channel_hold_last_policy_keeps_stale_value_visible() {
        let mut channel =
            LatestChannel::with_stale_config(StaleConfig::new(Some(10), StalePolicy::HoldLast));
        channel.publish_at(7u32, 100);

        let stale = channel.view_at(111);
        assert!(stale.present());
        assert!(stale.stale());
        assert_eq!(stale.as_ref(), Some(&7));
    }

    #[test]
    fn latest_channel_error_policy_keeps_value_for_shell_error_handling() {
        let mut channel =
            LatestChannel::with_stale_config(StaleConfig::new(Some(10), StalePolicy::Error));
        channel.publish_at(7u32, 100);

        let stale = channel.view_at(111);
        assert!(stale.present());
        assert!(stale.stale());
        assert_eq!(stale.as_ref(), Some(&7));
    }

    #[test]
    fn fifo_channel_marks_values_stale_after_max_age() {
        let mut channel = FifoChannel::with_stale_config(
            2,
            OverflowPolicy::DropOldest,
            StaleConfig::new(Some(10), StalePolicy::Warn),
        );
        assert_eq!(
            channel.push_at(7u32, 100),
            Ok(ChannelWriteOutcome::Accepted)
        );
        assert_eq!(
            channel.push_at(9u32, 100),
            Ok(ChannelWriteOutcome::Accepted)
        );

        let fresh_read = channel.pop_at(109);
        let fresh = fresh_read.view();
        assert!(fresh.present());
        assert!(!fresh.stale());
        assert_eq!(fresh.as_ref(), Some(&7));

        let stale_read = channel.pop_at(111);
        let stale = stale_read.view();
        assert!(stale.present());
        assert!(stale.stale());
        assert_eq!(stale.as_ref(), Some(&9));
    }

    #[test]
    fn fifo_channel_drop_policy_hides_stale_values() {
        let mut channel = FifoChannel::with_stale_config(
            1,
            OverflowPolicy::DropOldest,
            StaleConfig::new(Some(10), StalePolicy::Drop),
        );
        assert_eq!(
            channel.push_at(7u32, 100),
            Ok(ChannelWriteOutcome::Accepted)
        );

        let stale_read = channel.pop_at(111);
        let stale = stale_read.view();
        assert!(!stale.present());
        assert!(stale.stale());
        assert!(channel.is_empty());
    }

    #[test]
    fn fifo_channel_error_policy_keeps_stale_value_for_shell_error_handling() {
        let mut channel = FifoChannel::with_stale_config(
            1,
            OverflowPolicy::DropOldest,
            StaleConfig::new(Some(10), StalePolicy::Error),
        );
        assert_eq!(
            channel.push_at(7u32, 100),
            Ok(ChannelWriteOutcome::Accepted)
        );

        let stale_read = channel.pop_at(111);
        let stale = stale_read.view();
        assert!(stale.present());
        assert!(stale.stale());
        assert_eq!(stale.as_ref(), Some(&7));
    }

    #[test]
    fn fifo_channel_block_policy_reports_backpressure_without_overwriting_queue() {
        let mut channel = FifoChannel::new(1, OverflowPolicy::Block);
        assert_eq!(channel.push(1u32), Ok(ChannelWriteOutcome::Accepted));
        assert_eq!(channel.push(2u32), Ok(ChannelWriteOutcome::Backpressured));
        assert_eq!(channel.len(), 1);
        assert_eq!(channel.pop(), Some(1));
    }

    #[test]
    fn latest_channel_revision_advances_only_on_publish() {
        let mut channel = LatestChannel::new();
        assert_eq!(channel.revision(), 0);

        channel.publish(7u32);
        assert_eq!(channel.revision(), 1);
        assert_eq!(channel.view().as_ref(), Some(&7));
        assert_eq!(channel.revision(), 1);

        channel.publish_at(9u32, 5);
        assert_eq!(channel.revision(), 2);
    }

    #[test]
    fn fifo_channel_revision_advances_only_when_sample_enters_queue() {
        let mut channel = FifoChannel::new(1, OverflowPolicy::DropNewest);
        assert_eq!(channel.revision(), 0);

        assert_eq!(channel.push(1u32), Ok(ChannelWriteOutcome::Accepted));
        assert_eq!(channel.revision(), 1);
        assert_eq!(channel.push(2u32), Ok(ChannelWriteOutcome::DroppedNewest));
        assert_eq!(channel.revision(), 1);

        let _ = channel.pop();
        assert_eq!(channel.revision(), 1);
    }

    #[test]
    fn boundary_input_injects_latest_value_and_notifies_waiter() {
        let input = BoundaryInput::new();
        let waiter = crate::ScheduleWaiter::new();
        input.set_schedule_waiter(waiter.clone());
        let seen_generation = waiter.data_generation();

        let revision = input.inject_at(7u32, 100);
        let read = input.read_at(109);
        let view = read.view();

        assert_eq!(revision, 1);
        assert_eq!(read.revision(), 1);
        assert_eq!(view.as_ref(), Some(&7));
        assert!(!view.stale());
        assert!(waiter.data_generation() > seen_generation);
    }

    #[test]
    fn boundary_input_applies_stale_policy() {
        let input = BoundaryInput::with_stale_config(StaleConfig::new(Some(10), StalePolicy::Drop));

        input.inject_at(7u32, 100);
        let stale = input.read_at(111);
        let view = stale.view();

        assert_eq!(stale.revision(), 1);
        assert!(!view.present());
        assert!(view.stale());
    }

    #[test]
    fn boundary_output_sink_is_removed_when_guard_drops() {
        let output = BoundaryOutput::new();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(u32, Option<u64>)>::new()));
        let sink_seen = seen.clone();
        let guard = output.register_sink(move |value: &u32, published_at_ms| {
            sink_seen.lock().unwrap().push((*value, published_at_ms));
        });

        output.publish_at(&7, 100);
        drop(guard);
        output.publish_at(&9, 110);

        assert_eq!(output.sink_count(), 0);
        assert_eq!(*seen.lock().unwrap(), vec![(7, Some(100))]);
    }
}
