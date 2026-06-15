//! 把 record 时间线装配为运行时原生回放驱动 (v0.17.0)。
//!
//! 这是 record→replay 的运行时侧装配点：读取 MCAP 回放源，只保留目标在本图 boundary input
//! 名集合内的外部激励事件（回放不重放内部 channel，由 runtime 重新推导下游），映射为
//! [`ReplayEvent`] 并构造 [`ReplayDriver`]。生成 shell 在 simulated_replay / external_stepped
//! 时钟源下调用本模块，无需自行解析 MCAP。
//!
//! 与 [`crate::time_driver`] 的分工：time_driver 只做纯逻辑步进，不碰 MCAP；本模块负责
//! MCAP 解析、boundary 过滤与映射这层 IO/胶水。

use std::collections::BTreeSet;
use std::path::Path;

use flowrt_record::{ReplayTimelineEntry, read_replay_timeline_from_path};

use crate::time_driver::{ReplayDriver, ReplayEvent};

/// 把回放时间线条目过滤并映射为按时间升序的 boundary 激励事件。
///
/// 只保留 `target` 属于 `boundary_inputs` 的条目；其余（内部 channel sample）被忽略——确定性
/// 回放只重放外部边界激励。输入需已按时间升序（[`read_replay_timeline_from_path`] 已保证）。
pub fn boundary_replay_events(
    entries: impl IntoIterator<Item = ReplayTimelineEntry>,
    boundary_inputs: &BTreeSet<String>,
) -> Vec<ReplayEvent> {
    entries
        .into_iter()
        .filter(|entry| boundary_inputs.contains(&entry.target))
        .map(|entry| ReplayEvent {
            time_ms: entry.time_ms,
            target: entry.target,
            payload: entry.payload,
            sample_time_ms: None,
        })
        .collect()
}

/// 从 MCAP 回放源构造只含 boundary 激励的 [`ReplayDriver`]。
///
/// 读取失败（路径不存在、非 MCAP、envelope 损坏）返回 [`flowrt_record::RecordError`]，由调用方
/// 决定 fail-fast；不静默吞掉错误后用空时间线伪装成功回放。
pub fn replay_driver_from_mcap(
    path: &Path,
    boundary_inputs: &BTreeSet<String>,
) -> Result<ReplayDriver, flowrt_record::RecordError> {
    let entries = read_replay_timeline_from_path(path)?;
    Ok(ReplayDriver::new(boundary_replay_events(
        entries,
        boundary_inputs,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_driver::Step;
    use flowrt_record::{
        FlowrtMcapWriter, PayloadEncoding, RECORD_SCHEMA_VERSION, RecordEntity, RecordEntityKind,
        RecordEnvelope, RecordEventKind,
    };

    fn boundary_set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn channel_sample(name: &str, monotonic_ms: u64, payload: Vec<u8>) -> RecordEnvelope {
        RecordEnvelope {
            schema_version: RECORD_SCHEMA_VERSION,
            event_kind: RecordEventKind::ChannelSample,
            package: "demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 1,
            selfdesc_hash: "h".to_string(),
            monotonic_ns: monotonic_ms * 1_000_000,
            wall_unix_ns: 0,
            sequence: monotonic_ms,
            entity: RecordEntity {
                kind: RecordEntityKind::Channel,
                name: name.to_string(),
                instance: None,
                task: None,
                type_name: None,
            },
            payload_encoding: PayloadEncoding::CanonicalFrame,
            payload_schema: "Sample".to_string(),
            payload,
        }
    }

    #[test]
    fn boundary_replay_events_keeps_only_boundary_targets_in_order() {
        let entries = vec![
            ReplayTimelineEntry {
                time_ms: 5,
                target: "sample_in".to_string(),
                payload: vec![1],
            },
            ReplayTimelineEntry {
                time_ms: 6,
                target: "internal.channel".to_string(),
                payload: vec![2],
            },
            ReplayTimelineEntry {
                time_ms: 7,
                target: "sample_in".to_string(),
                payload: vec![3],
            },
        ];
        let events = boundary_replay_events(entries, &boundary_set(&["sample_in"]));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].time_ms, 5);
        assert_eq!(events[0].target, "sample_in");
        assert_eq!(events[0].payload, vec![1]);
        assert_eq!(events[1].time_ms, 7);
        assert!(events.iter().all(|event| event.sample_time_ms.is_none()));
    }

    #[test]
    fn replay_driver_from_mcap_reads_and_filters_boundary_stimuli() {
        let path = std::env::temp_dir().join(format!("flowrt-replay-{}.mcap", std::process::id()));
        {
            let mut writer = FlowrtMcapWriter::new(std::fs::File::create(&path).unwrap()).unwrap();
            let channel = writer
                .register_channel("samples", RecordEventKind::ChannelSample)
                .unwrap();
            writer
                .write_event(channel, &channel_sample("sample_in", 5, vec![1]))
                .unwrap();
            writer
                .write_event(channel, &channel_sample("internal.channel", 6, vec![2]))
                .unwrap();
            writer.flush().unwrap();
            writer.finish_into_inner().unwrap();
        }

        let mut driver = replay_driver_from_mcap(&path, &boundary_set(&["sample_in"])).unwrap();
        // 只有 boundary 事件参与：第一步命中 t=5 的 Data，且只暂存一个事件。
        assert_eq!(driver.step(Some(1000)), Step::Data);
        assert_eq!(driver.now_ms(), 5);
        assert_eq!(driver.take_pending_events().len(), 1);
        // 内部 channel 事件被过滤，时间线随即耗尽。
        assert_eq!(driver.step(Some(1000)), Step::Shutdown);

        let _ = std::fs::remove_file(&path);
    }
}
