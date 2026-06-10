use std::collections::BTreeMap;
use std::fmt;

/// frame descriptor 里的开放 metadata 键值。
pub type FrameMetadata = BTreeMap<String, String>;

/// RSDL frame descriptor message 的标准 fixed ABI 字段集合。
///
/// 该结构和 validator 要求的 message 字段一一对应，可作为 generated message 与
/// recorder/lease helper 之间的稳定中间形状。真实 payload 仍由 side-channel 管理。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameDescriptorFields {
    pub resource_id_hash: u64,
    pub slot: u32,
    pub generation: u64,
    pub size_bytes: u64,
    pub timestamp_unix_ns: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub format_id: u32,
    pub encoding_id: u32,
    pub flags: u32,
}

impl FrameDescriptorFields {
    pub fn resource_id_string(self) -> String {
        self.resource_id_hash.to_string()
    }

    pub fn slot_string(self) -> String {
        self.slot.to_string()
    }

    pub fn metadata(self) -> FrameMetadata {
        FrameMetadata::from([
            (
                "timestamp_unix_ns".to_string(),
                self.timestamp_unix_ns.to_string(),
            ),
            ("width".to_string(), self.width.to_string()),
            ("height".to_string(), self.height.to_string()),
            ("stride_bytes".to_string(), self.stride_bytes.to_string()),
            ("format_id".to_string(), self.format_id.to_string()),
            ("encoding_id".to_string(), self.encoding_id.to_string()),
            ("flags".to_string(), self.flags.to_string()),
        ])
    }

    pub fn to_descriptor(self) -> Result<FrameDescriptor, FrameDescriptorError> {
        FrameDescriptor::new(
            ResourceDescriptor::new(
                self.resource_id_string(),
                self.slot_string(),
                self.generation,
            ),
            self.size_bytes,
            self.format_id.to_string(),
            self.encoding_id.to_string(),
            self.metadata(),
        )
    }
}

/// side-channel 资源中的一个可寻址 payload slot。
///
/// descriptor 只说明 payload 位于哪个资源、哪个 slot、哪个 generation；它不代表
/// acquire 已成功，也不携带底层 SHM、相机或推理 SDK 句柄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDescriptor {
    resource_id: String,
    slot: String,
    generation: u64,
}

impl ResourceDescriptor {
    pub fn new(resource_id: impl Into<String>, slot: impl Into<String>, generation: u64) -> Self {
        Self {
            resource_id: resource_id.into(),
            slot: slot.into(),
            generation,
        }
    }

    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }

    pub fn slot(&self) -> &str {
        &self.slot
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// 普通 FlowRT channel 传递的 frame descriptor。
///
/// 大 payload 生命周期由 I/O boundary 或 external package 管理。该结构只携带
/// resource/slot/generation、大小、格式、编码和可观测 metadata。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDescriptor {
    resource: ResourceDescriptor,
    size_bytes: u64,
    format: String,
    encoding: String,
    metadata: FrameMetadata,
}

impl FrameDescriptor {
    pub fn new(
        resource: ResourceDescriptor,
        size_bytes: u64,
        format: impl Into<String>,
        encoding: impl Into<String>,
        metadata: FrameMetadata,
    ) -> Result<Self, FrameDescriptorError> {
        let format = format.into();
        let encoding = encoding.into();
        if resource.resource_id.is_empty() {
            return Err(FrameDescriptorError::InvalidField("resource_id"));
        }
        if resource.slot.is_empty() {
            return Err(FrameDescriptorError::InvalidField("slot"));
        }
        if size_bytes == 0 {
            return Err(FrameDescriptorError::InvalidField("size_bytes"));
        }
        if format.is_empty() {
            return Err(FrameDescriptorError::InvalidField("format"));
        }
        Ok(Self {
            resource,
            size_bytes,
            format,
            encoding,
            metadata,
        })
    }

    pub fn resource(&self) -> &ResourceDescriptor {
        &self.resource
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn format(&self) -> &str {
        &self.format
    }

    pub fn encoding(&self) -> &str {
        &self.encoding
    }

    pub fn metadata(&self) -> &FrameMetadata {
        &self.metadata
    }
}

/// descriptor 构造错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDescriptorError {
    InvalidField(&'static str),
}

impl fmt::Display for FrameDescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField(field) => {
                write!(formatter, "invalid frame descriptor field `{field}`")
            }
        }
    }
}

impl std::error::Error for FrameDescriptorError {}

/// side-channel lease 当前状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLeaseStatus {
    Attached,
    Acquired,
    Released,
    Expired,
    GenerationMismatch,
    Error,
}

/// side-channel lease 操作错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameLeaseError {
    Released,
    Expired,
    GenerationMismatch {
        descriptor_generation: u64,
        current_generation: u64,
    },
    Error(String),
}

/// 无硬件 side-channel lease primitive。
///
/// 该类型只表达 attach/acquire/release 的状态转换，不打开真实 SHM 或设备。
#[derive(Debug, Clone)]
pub struct FrameLease {
    descriptor: FrameDescriptor,
    current_generation: u64,
    status: FrameLeaseStatus,
    last_error: Option<String>,
}

impl FrameLease {
    pub fn attach(descriptor: FrameDescriptor, current_generation: u64) -> Self {
        Self {
            descriptor,
            current_generation,
            status: FrameLeaseStatus::Attached,
            last_error: None,
        }
    }

    pub fn descriptor(&self) -> &FrameDescriptor {
        &self.descriptor
    }

    pub fn status(&self) -> FrameLeaseStatus {
        self.status
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn acquire(&mut self, expected_generation: u64) -> Result<(), FrameLeaseError> {
        match self.status {
            FrameLeaseStatus::Released => return Err(FrameLeaseError::Released),
            FrameLeaseStatus::Expired => return Err(FrameLeaseError::Expired),
            FrameLeaseStatus::Error => {
                return Err(FrameLeaseError::Error(
                    self.last_error
                        .clone()
                        .unwrap_or_else(|| "error".to_string()),
                ));
            }
            FrameLeaseStatus::Attached
            | FrameLeaseStatus::Acquired
            | FrameLeaseStatus::GenerationMismatch => {}
        }

        if expected_generation != self.current_generation
            || self.descriptor.resource.generation != self.current_generation
        {
            self.status = FrameLeaseStatus::GenerationMismatch;
            return Err(FrameLeaseError::GenerationMismatch {
                descriptor_generation: self.descriptor.resource.generation,
                current_generation: self.current_generation,
            });
        }

        self.status = FrameLeaseStatus::Acquired;
        Ok(())
    }

    pub fn release(&mut self) -> Result<(), FrameLeaseError> {
        match self.status {
            FrameLeaseStatus::Expired => Err(FrameLeaseError::Expired),
            FrameLeaseStatus::Error => Err(FrameLeaseError::Error(
                self.last_error
                    .clone()
                    .unwrap_or_else(|| "error".to_string()),
            )),
            _ => {
                self.status = FrameLeaseStatus::Released;
                Ok(())
            }
        }
    }

    pub fn expire(&mut self) {
        self.status = FrameLeaseStatus::Expired;
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.last_error = Some(error.into());
        self.status = FrameLeaseStatus::Error;
    }
}
