use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use super::model::IntrospectionChannelSnapshot;

/// 数据面 probe 记录结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrospectionProbeRecord {
    pub recorded: bool,
    pub dropped: bool,
}
#[derive(Debug)]
struct ChannelProbeInner {
    observer_count: AtomicU64,
    dropped_samples: AtomicU64,
    published_count: AtomicU64,
    latest: Mutex<ChannelProbeLatest>,
}

#[derive(Debug, Default)]
struct ChannelProbeLatest {
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
    pub(super) fn new(max_payload_len: Option<usize>) -> Self {
        let payload = max_payload_len.map(Vec::with_capacity);
        Self {
            inner: Arc::new(ChannelProbeInner {
                observer_count: AtomicU64::new(0),
                dropped_samples: AtomicU64::new(0),
                published_count: AtomicU64::new(0),
                latest: Mutex::new(ChannelProbeLatest {
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

    /// 记录一次 channel 发布事件；只更新控制面计数，不拷贝 payload。
    pub fn record_publish_event(&self) {
        let _ = self.inner.published_count.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |current| Some(current.saturating_add(1)),
        );
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
        latest.published_at_ms = published_at_ms;
        IntrospectionProbeRecord {
            recorded: true,
            dropped: false,
        }
    }

    pub(super) fn force_record_bytes(&self, payload: Vec<u8>, published_at_ms: Option<u64>) {
        self.record_publish_event();
        let mut latest = self
            .inner
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        latest.payload = Some(payload);
        latest.published_at_ms = published_at_ms;
    }

    pub(super) fn snapshot(&self) -> IntrospectionChannelSnapshot {
        let latest = self
            .inner
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        IntrospectionChannelSnapshot {
            published_count: self.inner.published_count.load(Ordering::Acquire),
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
