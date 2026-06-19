#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::{Seek, Write};

use mcap::records::MessageHeader;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// FlowRT record envelope 的当前 schema 版本。
pub const RECORD_SCHEMA_VERSION: u16 = 1;
/// 写入 MCAP schema record 的稳定 schema 名称。
pub const RECORD_SCHEMA_NAME: &str = "flowrt.record.v1";
/// MCAP schema encoding。v1 envelope 使用 JSON Schema 描述。
pub const RECORD_SCHEMA_ENCODING: &str = "jsonschema";
/// MCAP message encoding。v1 envelope 以 JSON message payload 写入。
pub const RECORD_MESSAGE_ENCODING: &str = "json";
/// FrameDescriptor record payload 的稳定 JSON schema 名称。
pub const DESCRIPTOR_RECORD_SCHEMA_NAME: &str = "flowrt.descriptor.frame.v1";
/// MCAP 文件头尾 magic bytes。
pub const MCAP_MAGIC: &[u8] = b"\x89MCAP0\r\n";

const RECORD_SCHEMA_JSON: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "FlowRT RecordEnvelope v1",
  "type": "object",
  "required": [
    "schema_version",
    "event_kind",
    "package",
    "process",
    "runtime_pid",
    "selfdesc_hash",
    "monotonic_ns",
    "wall_unix_ns",
    "sequence",
    "entity",
    "payload_encoding",
    "payload_schema",
    "payload"
  ],
  "properties": {
    "schema_version": { "const": 1 },
    "event_kind": { "type": "string" },
    "package": { "type": "string" },
    "process": { "type": "string" },
    "runtime_pid": { "type": "integer", "minimum": 0 },
    "selfdesc_hash": { "type": "string" },
    "monotonic_ns": { "type": "integer", "minimum": 0 },
    "wall_unix_ns": { "type": "integer", "minimum": 0 },
    "sequence": { "type": "integer", "minimum": 0 },
    "entity": { "type": "object" },
    "payload_encoding": { "type": "string" },
    "payload_schema": { "type": "string" },
    "payload": { "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 255 } }
  }
}"#;

/// FlowRT record crate 的统一返回类型。
pub type RecordResult<T> = Result<T, RecordError>;

/// record envelope 序列化和 MCAP 写入阶段的结构化错误。
#[derive(Debug, Error)]
pub enum RecordError {
    #[error("record writer has already been closed")]
    WriterClosed,
    #[error(
        "record channel event kind {channel:?} does not match envelope event kind {envelope:?}"
    )]
    EventKindMismatch {
        channel: RecordEventKind,
        envelope: RecordEventKind,
    },
    #[error("record sequence {0} exceeds MCAP message header u32 range")]
    SequenceTooLarge(u64),
    #[error("serialize FlowRT record envelope: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("write MCAP record: {0}")]
    Mcap(#[from] mcap::McapError),
    #[error("read replay source: {0}")]
    Io(#[from] std::io::Error),
}

/// FlowRT 录制事件分类。
///
/// 这些分类覆盖 v0.6.0 record-only 录制范围。record 文件记录 FlowRT 语义事件，
/// 不是某个 backend 或外部生态的 topic bytes 别名。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordEventKind {
    ChannelSample,
    DescriptorEvent,
    ParamEvent,
    ServiceEvent,
    OperationEvent,
    SchedulerEvent,
    DiagnosticsEvent,
    ClockEvent,
    RuntimeEvent,
}

impl RecordEventKind {
    /// 当前 schema 版本支持的全部事件分类，顺序保持 canonical。
    pub const ALL: [Self; 9] = [
        Self::ChannelSample,
        Self::DescriptorEvent,
        Self::ParamEvent,
        Self::ServiceEvent,
        Self::OperationEvent,
        Self::SchedulerEvent,
        Self::DiagnosticsEvent,
        Self::ClockEvent,
        Self::RuntimeEvent,
    ];

    /// 返回 JSON/metadata 中使用的 snake_case 事件名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChannelSample => "channel_sample",
            Self::DescriptorEvent => "descriptor_event",
            Self::ParamEvent => "param_event",
            Self::ServiceEvent => "service_event",
            Self::OperationEvent => "operation_event",
            Self::SchedulerEvent => "scheduler_event",
            Self::DiagnosticsEvent => "diagnostics_event",
            Self::ClockEvent => "clock_event",
            Self::RuntimeEvent => "runtime_event",
        }
    }
}

/// record 事件关联的 FlowRT 实体种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordEntityKind {
    Channel,
    Resource,
    Param,
    Service,
    Operation,
    Task,
    Lane,
    Diagnostic,
    Clock,
    Runtime,
    Process,
}

/// descriptor record 事件中的 lease/status 语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DescriptorRecordStatus {
    Attached,
    Acquired,
    Released,
    Expired,
    GenerationMismatch,
    Error,
}

/// `descriptor_event` 的 JSON payload。
///
/// 该 payload 只记录 descriptor 和 side-channel 事件状态，不携带或默认复制真实 frame
/// bytes。`payload_recording` 只有在上层显式 opt-in 后才应为 true。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptorRecordPayload {
    pub resource_id: String,
    pub slot: String,
    pub generation: u64,
    pub size_bytes: u64,
    pub format: String,
    pub encoding: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub status: DescriptorRecordStatus,
    #[serde(default)]
    pub payload_recording: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_artifact: Option<DescriptorPayloadArtifact>,
}

/// descriptor payload 录制的 artifact 元数据。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptorPayloadArtifact {
    pub artifact_ref: String,
    pub content_hash: String,
    pub size_bytes: u64,
}

/// record payload 的编码语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadEncoding {
    RawAbi,
    CanonicalFrame,
    Json,
}

impl PayloadEncoding {
    /// 返回 JSON/metadata 中使用的 snake_case 编码名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawAbi => "raw_abi",
            Self::CanonicalFrame => "canonical_frame",
            Self::Json => "json",
        }
    }
}

/// record 事件关联的实体身份。
///
/// `name` 应使用 Contract IR 或 runtime self-description 中的 canonical 名称。
/// `instance`、`task` 和 `type_name` 是可选补充索引，不替代 `name`。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordEntity {
    pub kind: RecordEntityKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
}

/// FlowRT record v1 的语言无关事件 envelope。
///
/// 时间字段同时保留单调时间和 wall time：MCAP header 使用 `wall_unix_ns` 作为
/// 展示时间线，后续确定性调试可以读取 envelope 内的 `monotonic_ns`
/// 建立运行时相对顺序。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordEnvelope {
    pub schema_version: u16,
    pub event_kind: RecordEventKind,
    pub package: String,
    pub process: String,
    pub runtime_pid: u32,
    pub selfdesc_hash: String,
    pub monotonic_ns: u64,
    /// sensor sample-time（纳秒）。声明了 timestamp 源的 channel sample 在录制时填充，用作
    /// event-time 回放时钟；其余事件为空。v0.18.0 引入，跨语言可选字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_time_ns: Option<u64>,
    pub wall_unix_ns: u64,
    pub sequence: u64,
    pub entity: RecordEntity,
    pub payload_encoding: PayloadEncoding,
    pub payload_schema: String,
    pub payload: Vec<u8>,
}

impl RecordEnvelope {
    /// 将 envelope 编码为 MCAP message payload。
    ///
    /// v1 使用 JSON 是为了让 C++ tap、CLI 和后续 Python 工具不依赖 Rust 私有布局。
    /// 二进制 payload 在 JSON 中以字节数组表达；这不是最终性能路径，后续可在保持
    /// schema_version 的前提下增加 versioned binary encoding。
    pub fn to_json_bytes(&self) -> RecordResult<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }
}

/// 回放时间线的一条记录：在某时刻把一段 wire payload 注入某个目标 channel/boundary。
///
/// 这是 record→replay 的中间表示：reader 只做「读 MCAP、过滤 channel sample、按时间排序」，
/// 不依赖 runtime 类型；生成 shell 再把它映射为 runtime 回放事件并注入对应 boundary input。
///
/// 同时是 JSONL 回放源的行 schema（见 [`write_replay_timeline_jsonl`]）：`payload` 以 JSON
/// 整数数组承载。后续 sensor event-time 可在保持向后兼容的前提下追加 `sample_time_ms` 字段，
/// 当前 reader 默认忽略未知字段。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayTimelineEntry {
    /// 事件进入 runtime 的逻辑毫秒时间（由 envelope `monotonic_ns` 推导，即 receive-time）。
    pub time_ms: u64,
    /// 注入目标的 canonical 名称（envelope `entity.name`）。
    pub target: String,
    /// 注入的 wire payload 字节。
    pub payload: Vec<u8>,
    /// sensor sample-time（毫秒）。声明了 timestamp 源的消息在录制时填充；用作 event-time
    /// 回放的逻辑时钟（缺省回退到 `time_ms` receive-time）。v0.18.0 引入。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_time_ms: Option<u64>,
}

impl ReplayTimelineEntry {
    /// event-time 回放使用的有效逻辑时间：有 sample-time 用之，否则回退 receive-time。
    pub fn effective_time_ms(&self) -> u64 {
        self.sample_time_ms.unwrap_or(self.time_ms)
    }
}

/// 从 MCAP 字节读出按时间升序的回放时间线（只取 `ChannelSample` 事件）。
///
/// 确定性回放只重放数据样本，按 `(monotonic_ns, sequence)` 稳定排序；毫秒时间用于逻辑时钟
/// 步进。非 channel sample 事件（scheduler、param、clock 等观测事件）被跳过，不参与注入。
pub fn read_replay_timeline(data: &[u8]) -> RecordResult<Vec<ReplayTimelineEntry>> {
    let mut ordered: Vec<(u64, u64, ReplayTimelineEntry)> = Vec::new();
    for message in mcap::MessageStream::new(data)? {
        let message = message?;
        let envelope: RecordEnvelope = serde_json::from_slice(&message.data)?;
        if envelope.event_kind != RecordEventKind::ChannelSample {
            continue;
        }
        let entry = ReplayTimelineEntry {
            time_ms: envelope.monotonic_ns / 1_000_000,
            target: envelope.entity.name,
            payload: envelope.payload,
            sample_time_ms: envelope.sample_time_ns.map(|ns| ns / 1_000_000),
        };
        // 按 effective time（sample-time 优先，否则 receive-time）稳定排序，使 event-time 回放
        // 时间线按 sensor 采集时刻有序。
        ordered.push((entry.effective_time_ms(), envelope.sequence, entry));
    }
    ordered.sort_by_key(|(effective_ms, sequence, _)| (*effective_ms, *sequence));
    Ok(ordered.into_iter().map(|(_, _, entry)| entry).collect())
}

/// 读取 MCAP 文件并解析回放时间线。
pub fn read_replay_timeline_from_path(
    path: &std::path::Path,
) -> RecordResult<Vec<ReplayTimelineEntry>> {
    let data = std::fs::read(path)?;
    read_replay_timeline(&data)
}

/// 把回放时间线写为 line-delimited JSON（每行一条 [`ReplayTimelineEntry`]）。
///
/// 这是 C++ runtime 可直接解析的回放源格式。C++ 没有 MCAP 解析能力，`flowrt run --replay`
/// 在启动 C++ 生成 shell 前先用本函数把 MCAP 规范化为 JSONL 时间线。Rust runtime 仍直读
/// MCAP，故本格式只是跨语言回放源的最小公共承载，不引入第二套 MCAP 解析实现。
pub fn write_replay_timeline_jsonl<W: Write>(
    writer: &mut W,
    entries: &[ReplayTimelineEntry],
) -> RecordResult<()> {
    for entry in entries {
        serde_json::to_writer(&mut *writer, entry)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

/// 把回放时间线写入 JSONL 文件。
pub fn write_replay_timeline_jsonl_to_path(
    path: &std::path::Path,
    entries: &[ReplayTimelineEntry],
) -> RecordResult<()> {
    let mut file = std::fs::File::create(path)?;
    write_replay_timeline_jsonl(&mut file, entries)?;
    file.flush()?;
    Ok(())
}

/// 解析 line-delimited JSON 回放时间线。空行被跳过；输入应保持写入时的时间升序。
pub fn read_replay_timeline_jsonl(data: &str) -> RecordResult<Vec<ReplayTimelineEntry>> {
    let mut entries = Vec::new();
    for line in data.lines() {
        if line.trim().is_empty() {
            continue;
        }
        entries.push(serde_json::from_str(line)?);
    }
    Ok(entries)
}

/// 读取并解析 JSONL 回放时间线文件。
pub fn read_replay_timeline_jsonl_from_path(
    path: &std::path::Path,
) -> RecordResult<Vec<ReplayTimelineEntry>> {
    let data = std::fs::read_to_string(path)?;
    read_replay_timeline_jsonl(&data)
}

/// 已注册的 MCAP channel 句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordChannel {
    id: u16,
    event_kind: RecordEventKind,
}

impl RecordChannel {
    /// MCAP channel id。
    pub const fn id(self) -> u16 {
        self.id
    }

    /// 该 channel 承载的 FlowRT record event kind。
    pub const fn event_kind(self) -> RecordEventKind {
        self.event_kind
    }
}

/// FlowRT record MCAP writer 的最小封装。
///
/// writer 只负责 schema/channel 注册和 envelope 写入，不持有 runtime tap 状态，
/// 也不实现 CLI 生命周期。后续 recorder runtime tap 可以直接复用该类型。
pub struct FlowrtMcapWriter<W: Write + Seek> {
    writer: Option<mcap::Writer<W>>,
    schema_id: u16,
}

impl<W: Write + Seek> FlowrtMcapWriter<W> {
    /// 创建 writer 并注册 FlowRT record v1 schema。
    pub fn new(writer: W) -> RecordResult<Self> {
        let mut writer = mcap::Writer::new(writer)?;
        let schema_id = writer.add_schema(
            RECORD_SCHEMA_NAME,
            RECORD_SCHEMA_ENCODING,
            RECORD_SCHEMA_JSON.as_bytes(),
        )?;

        Ok(Self {
            writer: Some(writer),
            schema_id,
        })
    }

    /// 注册一个承载指定事件分类的 MCAP channel。
    pub fn register_channel(
        &mut self,
        topic: impl AsRef<str>,
        event_kind: RecordEventKind,
    ) -> RecordResult<RecordChannel> {
        let schema_id = self.schema_id;
        let mut metadata = BTreeMap::new();
        metadata.insert("flowrt.schema".to_string(), RECORD_SCHEMA_NAME.to_string());
        metadata.insert(
            "flowrt.event_kind".to_string(),
            event_kind.as_str().to_string(),
        );

        let id = self.writer_mut()?.add_channel(
            schema_id,
            topic.as_ref(),
            RECORD_MESSAGE_ENCODING,
            &metadata,
        )?;
        Ok(RecordChannel { id, event_kind })
    }

    /// 将一个 FlowRT record envelope 写入已注册 channel。
    pub fn write_event(
        &mut self,
        channel: RecordChannel,
        envelope: &RecordEnvelope,
    ) -> RecordResult<()> {
        if channel.event_kind != envelope.event_kind {
            return Err(RecordError::EventKindMismatch {
                channel: channel.event_kind,
                envelope: envelope.event_kind,
            });
        }

        let data = envelope.to_json_bytes()?;
        let sequence = envelope
            .sequence
            .try_into()
            .map_err(|_| RecordError::SequenceTooLarge(envelope.sequence))?;
        let header = MessageHeader {
            channel_id: channel.id,
            sequence,
            log_time: envelope.wall_unix_ns,
            publish_time: envelope.wall_unix_ns,
        };

        self.writer_mut()?.write_to_known_channel(&header, &data)?;
        Ok(())
    }

    /// flush 当前 MCAP chunk 和底层 writer。
    pub fn flush(&mut self) -> RecordResult<()> {
        self.writer_mut()?.flush()?;
        Ok(())
    }

    /// 完成 MCAP 文件并返回底层 writer。
    pub fn finish_into_inner(mut self) -> RecordResult<W> {
        let mut writer = self.writer.take().ok_or(RecordError::WriterClosed)?;
        writer.finish()?;
        Ok(writer.into_inner())
    }

    fn writer_mut(&mut self) -> RecordResult<&mut mcap::Writer<W>> {
        self.writer.as_mut().ok_or(RecordError::WriterClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_replay_timeline_jsonl_roundtrips_entries() {
        let entries = vec![
            ReplayTimelineEntry {
                time_ms: 5,
                target: "sample_in".to_string(),
                payload: vec![1, 2, 3],
                sample_time_ms: Some(50),
            },
            ReplayTimelineEntry {
                time_ms: 7,
                target: "imu_in".to_string(),
                payload: vec![],
                sample_time_ms: None,
            },
        ];
        let mut buffer = Vec::new();
        write_replay_timeline_jsonl(&mut buffer, &entries).unwrap();
        // 每条 entry 占一行。
        assert_eq!(buffer.iter().filter(|byte| **byte == b'\n').count(), 2);
        let text = String::from_utf8(buffer).unwrap();
        assert_eq!(read_replay_timeline_jsonl(&text).unwrap(), entries);
    }

    #[test]
    fn read_replay_timeline_jsonl_skips_blank_lines_and_tolerates_unknown_fields() {
        // 未知 `sample_time_ms` 字段被忽略，验证 0.18.0 sensor event-time 的向前兼容承诺。
        let text = "{\"time_ms\":1,\"target\":\"a\",\"payload\":[9],\"sample_time_ms\":42}\n\n";
        let parsed = read_replay_timeline_jsonl(text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].time_ms, 1);
        assert_eq!(parsed[0].target, "a");
        assert_eq!(parsed[0].payload, vec![9]);
    }
}
