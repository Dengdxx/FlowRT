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
    ClockEvent,
    RuntimeEvent,
}

impl RecordEventKind {
    /// 当前 schema 版本支持的全部事件分类，顺序保持 canonical。
    pub const ALL: [Self; 8] = [
        Self::ChannelSample,
        Self::DescriptorEvent,
        Self::ParamEvent,
        Self::ServiceEvent,
        Self::OperationEvent,
        Self::SchedulerEvent,
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
