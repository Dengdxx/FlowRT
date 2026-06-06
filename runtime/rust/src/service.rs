//! FlowRT Service core primitives。
//!
//! 本模块定义跨 Rust/C++ 一致的 Service 运行时基础类型：错误码、结果类型、请求标识、
//! 超时/截止时间和 canonical service frame。这些类型是 request/response 语义的协议边界，
//! 不是 backend transport API。
//!
//! canonical service frame 使用 little-endian 固定 header + 变长 tail 的编码方式。
//! header 固定 80 字节，变长字段（payload、错误消息）通过 tail 中的 VarSpan 描述符寻址。
//! inproc 可以做 typed/direct dispatch，但其语义必须与 canonical frame 等价。

use crate::frame::{FrameDecoder, VarSpan, append_tail_block};
use crate::wire::WireCodecError;

/// service frame 魔数，ASCII "FRVS" = 0x46525653，little-endian 存储为 0x53525646。
pub const SERVICE_FRAME_MAGIC: u32 = 0x5352_5646;

/// service frame 协议版本，当前为 1。
pub const SERVICE_FRAME_VERSION: u16 = 1;

/// service frame 固定 header 字节数。
pub const SERVICE_FRAME_HEADER_SIZE: usize = 80;

/// Service 错误分类。
///
/// 错误码数值稳定，跨 Rust/C++/C ABI 保持一致。该枚举独立于 `Status`（调度状态）
/// 和 `ChannelError`（通道错误），专门描述 request/response 语义中的失败原因。
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceError {
    /// 请求成功完成。
    Ok = 0,
    /// 请求超时。
    Timeout = 1,
    /// 目标服务不可用。
    Unavailable = 2,
    /// 服务繁忙，请求被限流或排队溢出。
    Busy = 3,
    /// 请求被服务端主动拒绝。
    Rejected = 4,
    /// 请求被取消。
    Cancelled = 5,
    /// 截止时间已过。
    DeadlineExceeded = 6,
    /// 协议层错误（magic/version 不匹配、帧格式非法等）。
    Protocol = 7,
    /// 后端传输错误。
    Backend = 8,
    /// 执行会导致死锁。
    WouldDeadlock = 9,
    /// 用户 handler 自身返回的业务错误。
    HandlerError = 10,
}

impl ServiceError {
    /// 从 ABI u16 值解析错误码，未知值返回 `None`。
    pub const fn from_abi(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Ok),
            1 => Some(Self::Timeout),
            2 => Some(Self::Unavailable),
            3 => Some(Self::Busy),
            4 => Some(Self::Rejected),
            5 => Some(Self::Cancelled),
            6 => Some(Self::DeadlineExceeded),
            7 => Some(Self::Protocol),
            8 => Some(Self::Backend),
            9 => Some(Self::WouldDeadlock),
            10 => Some(Self::HandlerError),
            _ => None,
        }
    }

    /// 判断该错误码是否表示成功。
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "Ok"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Unavailable => write!(f, "Unavailable"),
            Self::Busy => write!(f, "Busy"),
            Self::Rejected => write!(f, "Rejected"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::DeadlineExceeded => write!(f, "DeadlineExceeded"),
            Self::Protocol => write!(f, "Protocol"),
            Self::Backend => write!(f, "Backend"),
            Self::WouldDeadlock => write!(f, "WouldDeadlock"),
            Self::HandlerError => write!(f, "HandlerError"),
        }
    }
}

/// Service request/response 结果类型。
///
/// `ServiceResult<T>` 携带 `ServiceError` 错误码和可选的成功值。它不能把 runtime/service
/// 错误塞进普通 `Status`，是 Service 语义的专用返回类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceResult<T> {
    /// 请求成功，携带响应值。
    Ok(T),
    /// 请求失败，携带错误码和可选错误消息。
    Err(ServiceError, Option<String>),
}

impl<T> ServiceResult<T> {
    /// 构造成功结果。
    pub const fn ok(value: T) -> Self {
        Self::Ok(value)
    }

    /// 构造失败结果（无消息）。
    pub const fn err(code: ServiceError) -> Self {
        Self::Err(code, None)
    }

    /// 构造失败结果（带消息）。
    pub fn err_with_message(code: ServiceError, message: impl Into<String>) -> Self {
        Self::Err(code, Some(message.into()))
    }

    /// 判断是否成功。
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// 判断是否失败。
    pub const fn is_err(&self) -> bool {
        matches!(self, Self::Err(_, _))
    }

    /// 获取错误码，成功时返回 `ServiceError::Ok`。
    pub const fn error_code(&self) -> ServiceError {
        match self {
            Self::Ok(_) => ServiceError::Ok,
            Self::Err(code, _) => *code,
        }
    }

    /// 借用错误消息。
    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Ok(_) => None,
            Self::Err(_, msg) => msg.as_deref(),
        }
    }

    /// 转换为 `Option<T>`，丢弃错误信息。
    pub fn ok_value(self) -> Option<T> {
        match self {
            Self::Ok(v) => Some(v),
            Self::Err(_, _) => None,
        }
    }
}

impl<T> std::fmt::Display for ServiceResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok(_) => write!(f, "Ok"),
            Self::Err(code, Some(msg)) => write!(f, "{}: {}", code, msg),
            Self::Err(code, None) => write!(f, "{}", code),
        }
    }
}

/// 客户端会话标识。
///
/// 每个 service client 实例在创建时分配一个唯一 session id，用于关联该 client 发出的
/// 所有请求。推荐使用随机 u64 或进程级自增计数器。
pub type ClientSessionId = u64;

/// 单调递增请求序号。
///
/// 在同一 client session 内单调递增，用于去重和乱序检测。
pub type SequenceNum = u64;

/// Service 请求标识。
///
/// 三元组唯一标识一个 service request：client session + sequence + service。
/// service_id 使用 canonical service name 的 FNV-1a 64-bit hash。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId {
    /// 发起请求的 client session 标识。
    pub session_id: ClientSessionId,
    /// 该 session 内的单调递增序号。
    pub sequence: SequenceNum,
    /// 目标 service 的 canonical name hash（FNV-1a 64-bit）。
    pub service_id: u64,
}

impl RequestId {
    /// 构造请求标识。
    pub const fn new(session_id: ClientSessionId, sequence: SequenceNum, service_id: u64) -> Self {
        Self {
            session_id,
            sequence,
            service_id,
        }
    }
}

/// FNV-1a 64-bit hash，用于从 canonical service name 生成 service_id。
///
/// 该算法选择理由：简单、无依赖、确定性、跨语言易实现。不用于安全场景。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Service 请求/响应截止时间。
///
/// 使用单调时钟毫秒表示绝对截止时间。ABI 中同时携带 timeout_ms（相对超时）和
/// absolute_deadline_ms（绝对截止），由调用方在发送时计算绝对值。
///
/// 默认不允许无界等待：timeout_ms 为 0 表示非法值，解码时应报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    /// 相对超时毫秒数。0 表示非法（不允许无界等待）。
    pub timeout_ms: u64,
    /// 绝对截止时间（单调时钟毫秒）。解码时从 timeout_ms + 当前时间推导。
    pub absolute_deadline_ms: u64,
}

impl Deadline {
    /// 构造截止时间。
    ///
    /// `timeout_ms` 必须大于 0。`now_monotonic_ms` 为当前单调时钟毫秒。
    pub fn new(timeout_ms: u64, now_monotonic_ms: u64) -> Option<Self> {
        if timeout_ms == 0 {
            return None;
        }
        Some(Self {
            timeout_ms,
            absolute_deadline_ms: now_monotonic_ms.saturating_add(timeout_ms),
        })
    }

    /// 判断是否已过期。
    pub fn expired(self, now_monotonic_ms: u64) -> bool {
        now_monotonic_ms >= self.absolute_deadline_ms
    }
}

/// Service ID 计算中的 canonical name hash 字段偏移（header 内）。
///
/// Service frame 固定 header 80 字节，布局如下（所有字段 little-endian）：
///
/// ```text
/// offset  size  field
/// 0       4     magic (u32, 0x53525646)
/// 4       2     version (u16, 当前 1)
/// 6       2     error_code (u16, ServiceError ABI 编码)
/// 8       8     service_id (u64, FNV-1a hash of canonical name)
/// 16      8     session_id (u64, client session)
/// 24      8     sequence (u64, monotonic sequence)
/// 32      8     correlation_id (u64, 跨系统关联，0 表示无)
/// 40      8     timeout_ms (u64, 相对超时，0 为非法)
/// 48      8     absolute_deadline_ms (u64, 单调时钟绝对截止)
/// 56      8     schema_hash (u64, 预留，0 表示未使用)
/// 64      8     payload_span (VarSpan, tail 中的 payload 位置)
/// 72      8     error_msg_span (VarSpan, tail 中的错误消息位置)
/// ```
///
/// 变长字段（payload、error message）存储在 tail 中，通过 header 中的 VarSpan 描述符寻址。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFrameHeader {
    pub magic: u32,
    pub version: u16,
    pub error_code: u16,
    pub service_id: u64,
    pub session_id: u64,
    pub sequence: u64,
    pub correlation_id: u64,
    pub timeout_ms: u64,
    pub absolute_deadline_ms: u64,
    pub schema_hash: u64,
    pub payload_span: VarSpan,
    pub error_msg_span: VarSpan,
}

impl ServiceFrameHeader {
    /// 构造请求帧 header。
    pub fn request(
        request_id: RequestId,
        deadline: Deadline,
        correlation_id: u64,
        schema_hash: u64,
    ) -> Self {
        Self {
            magic: SERVICE_FRAME_MAGIC,
            version: SERVICE_FRAME_VERSION,
            error_code: ServiceError::Ok as u16,
            service_id: request_id.service_id,
            session_id: request_id.session_id,
            sequence: request_id.sequence,
            correlation_id,
            timeout_ms: deadline.timeout_ms,
            absolute_deadline_ms: deadline.absolute_deadline_ms,
            schema_hash,
            payload_span: VarSpan::default(),
            error_msg_span: VarSpan::default(),
        }
    }

    /// 构造响应帧 header。
    pub fn response(
        request_id: RequestId,
        deadline: Deadline,
        correlation_id: u64,
        schema_hash: u64,
        error_code: ServiceError,
    ) -> Self {
        Self {
            magic: SERVICE_FRAME_MAGIC,
            version: SERVICE_FRAME_VERSION,
            error_code: error_code as u16,
            service_id: request_id.service_id,
            session_id: request_id.session_id,
            sequence: request_id.sequence,
            correlation_id,
            timeout_ms: deadline.timeout_ms,
            absolute_deadline_ms: deadline.absolute_deadline_ms,
            schema_hash,
            payload_span: VarSpan::default(),
            error_msg_span: VarSpan::default(),
        }
    }

    /// 将 header 编码到 80 字节 buffer。
    pub fn encode(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != SERVICE_FRAME_HEADER_SIZE {
            return Err(WireCodecError::wrong_size(
                SERVICE_FRAME_HEADER_SIZE,
                output.len(),
            ));
        }
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.version.to_le_bytes());
        output[6..8].copy_from_slice(&self.error_code.to_le_bytes());
        output[8..16].copy_from_slice(&self.service_id.to_le_bytes());
        output[16..24].copy_from_slice(&self.session_id.to_le_bytes());
        output[24..32].copy_from_slice(&self.sequence.to_le_bytes());
        output[32..40].copy_from_slice(&self.correlation_id.to_le_bytes());
        output[40..48].copy_from_slice(&self.timeout_ms.to_le_bytes());
        output[48..56].copy_from_slice(&self.absolute_deadline_ms.to_le_bytes());
        output[56..64].copy_from_slice(&self.schema_hash.to_le_bytes());
        self.payload_span.encode(&mut output[64..72])?;
        self.error_msg_span.encode(&mut output[72..80])?;
        Ok(())
    }

    /// 从 80 字节 buffer 解码 header。
    pub fn decode(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != SERVICE_FRAME_HEADER_SIZE {
            return Err(WireCodecError::wrong_size(
                SERVICE_FRAME_HEADER_SIZE,
                input.len(),
            ));
        }
        let magic = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
        if magic != SERVICE_FRAME_MAGIC {
            return Err(WireCodecError::invalid_frame(
                "service frame magic mismatch",
            ));
        }
        let version = u16::from_le_bytes([input[4], input[5]]);
        if version != SERVICE_FRAME_VERSION {
            return Err(WireCodecError::invalid_frame(
                "service frame version mismatch",
            ));
        }
        let error_code = u16::from_le_bytes([input[6], input[7]]);
        if ServiceError::from_abi(error_code).is_none() {
            return Err(WireCodecError::invalid_frame(
                "service frame error code is unknown",
            ));
        }
        let timeout_ms = u64::from_le_bytes([
            input[40], input[41], input[42], input[43], input[44], input[45], input[46], input[47],
        ]);
        if timeout_ms == 0 {
            return Err(WireCodecError::invalid_frame(
                "service frame timeout_ms must be greater than zero",
            ));
        }

        Ok(Self {
            magic,
            version,
            error_code,
            service_id: u64::from_le_bytes([
                input[8], input[9], input[10], input[11], input[12], input[13], input[14],
                input[15],
            ]),
            session_id: u64::from_le_bytes([
                input[16], input[17], input[18], input[19], input[20], input[21], input[22],
                input[23],
            ]),
            sequence: u64::from_le_bytes([
                input[24], input[25], input[26], input[27], input[28], input[29], input[30],
                input[31],
            ]),
            correlation_id: u64::from_le_bytes([
                input[32], input[33], input[34], input[35], input[36], input[37], input[38],
                input[39],
            ]),
            timeout_ms,
            absolute_deadline_ms: u64::from_le_bytes([
                input[48], input[49], input[50], input[51], input[52], input[53], input[54],
                input[55],
            ]),
            schema_hash: u64::from_le_bytes([
                input[56], input[57], input[58], input[59], input[60], input[61], input[62],
                input[63],
            ]),
            payload_span: VarSpan::decode(&input[64..72])?,
            error_msg_span: VarSpan::decode(&input[72..80])?,
        })
    }
}

/// 编码完整的 service frame（header + tail）到 byte vector。
///
/// `payload` 为请求/响应体字节，`error_msg` 为可选错误消息字节。
pub fn encode_service_frame(
    header: &ServiceFrameHeader,
    payload: &[u8],
    error_msg: &[u8],
) -> Result<Vec<u8>, WireCodecError> {
    let mut tail = Vec::new();
    let payload_span = append_tail_block(&mut tail, payload)?;
    let error_msg_span = append_tail_block(&mut tail, error_msg)?;

    let mut final_header = header.clone();
    final_header.payload_span = payload_span;
    final_header.error_msg_span = error_msg_span;

    let mut frame = vec![0u8; SERVICE_FRAME_HEADER_SIZE + tail.len()];
    final_header.encode(&mut frame[..SERVICE_FRAME_HEADER_SIZE])?;
    frame[SERVICE_FRAME_HEADER_SIZE..].copy_from_slice(&tail);
    Ok(frame)
}

/// 解码完整的 service frame，返回 header、payload 和 error message。
pub fn decode_service_frame(
    frame: &[u8],
) -> Result<(ServiceFrameHeader, Vec<u8>, Vec<u8>), WireCodecError> {
    if frame.len() < SERVICE_FRAME_HEADER_SIZE {
        return Err(WireCodecError::wrong_size(
            SERVICE_FRAME_HEADER_SIZE,
            frame.len(),
        ));
    }
    let header = ServiceFrameHeader::decode(&frame[..SERVICE_FRAME_HEADER_SIZE])?;
    let tail = &frame[SERVICE_FRAME_HEADER_SIZE..];
    let mut decoder = FrameDecoder::new(tail);
    let payload = decoder.read_block(header.payload_span)?.to_vec();
    let error_msg = decoder.read_block(header.error_msg_span)?.to_vec();
    decoder.finish()?;
    Ok((header, payload, error_msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_error_abi_values_are_stable() {
        // 防止错误码数值漂移
        assert_eq!(ServiceError::Ok as u16, 0);
        assert_eq!(ServiceError::Timeout as u16, 1);
        assert_eq!(ServiceError::Unavailable as u16, 2);
        assert_eq!(ServiceError::Busy as u16, 3);
        assert_eq!(ServiceError::Rejected as u16, 4);
        assert_eq!(ServiceError::Cancelled as u16, 5);
        assert_eq!(ServiceError::DeadlineExceeded as u16, 6);
        assert_eq!(ServiceError::Protocol as u16, 7);
        assert_eq!(ServiceError::Backend as u16, 8);
        assert_eq!(ServiceError::WouldDeadlock as u16, 9);
        assert_eq!(ServiceError::HandlerError as u16, 10);
    }

    #[test]
    fn service_error_from_abi_roundtrip() {
        for code in 0..=10u16 {
            let error = ServiceError::from_abi(code).unwrap();
            assert_eq!(error as u16, code);
        }
        assert!(ServiceError::from_abi(11).is_none());
        assert!(ServiceError::from_abi(u16::MAX).is_none());
    }

    #[test]
    fn service_error_display() {
        assert_eq!(ServiceError::Ok.to_string(), "Ok");
        assert_eq!(ServiceError::Timeout.to_string(), "Timeout");
        assert_eq!(ServiceError::HandlerError.to_string(), "HandlerError");
    }

    #[test]
    fn service_result_ok_and_err() {
        let ok: ServiceResult<u32> = ServiceResult::ok(42);
        assert!(ok.is_ok());
        assert!(!ok.is_err());
        assert_eq!(ok.error_code(), ServiceError::Ok);
        assert!(ok.error_message().is_none());
        assert_eq!(ok.ok_value(), Some(42));

        let err: ServiceResult<u32> =
            ServiceResult::err_with_message(ServiceError::Timeout, "request timed out");
        assert!(!err.is_ok());
        assert!(err.is_err());
        assert_eq!(err.error_code(), ServiceError::Timeout);
        assert_eq!(err.error_message(), Some("request timed out"));
        assert!(err.ok_value().is_none());

        let err_no_msg: ServiceResult<u32> = ServiceResult::err(ServiceError::Busy);
        assert_eq!(err_no_msg.error_code(), ServiceError::Busy);
        assert!(err_no_msg.error_message().is_none());
    }

    #[test]
    fn service_result_display() {
        let ok: ServiceResult<u32> = ServiceResult::ok(1);
        assert_eq!(ok.to_string(), "Ok");

        let err: ServiceResult<u32> = ServiceResult::err(ServiceError::Backend);
        assert_eq!(err.to_string(), "Backend");

        let err_msg: ServiceResult<u32> =
            ServiceResult::err_with_message(ServiceError::HandlerError, "division by zero");
        assert_eq!(err_msg.to_string(), "HandlerError: division by zero");
    }

    #[test]
    fn request_id_construction() {
        let id = RequestId::new(0xDEAD_BEEF, 42, 0x1234_5678);
        assert_eq!(id.session_id, 0xDEAD_BEEF);
        assert_eq!(id.sequence, 42);
        assert_eq!(id.service_id, 0x1234_5678);
    }

    #[test]
    fn fnv1a64_deterministic() {
        // FNV-1a 64-bit offset basis
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        // 同一名称总是得到同一 hash
        let name = "my_robot::imu_service";
        assert_eq!(fnv1a64(name.as_bytes()), fnv1a64(name.as_bytes()));
        // 不同输入产生不同 hash
        assert_ne!(fnv1a64(b"a"), fnv1a64(b"b"));
    }

    #[test]
    fn deadline_rejects_zero_timeout() {
        assert!(Deadline::new(0, 1000).is_none());
    }

    #[test]
    fn deadline_computes_absolute() {
        let d = Deadline::new(500, 1000).unwrap();
        assert_eq!(d.timeout_ms, 500);
        assert_eq!(d.absolute_deadline_ms, 1500);
        assert!(!d.expired(1499));
        assert!(d.expired(1500));
        assert!(d.expired(2000));
    }

    #[test]
    fn service_frame_header_size_is_80() {
        assert_eq!(SERVICE_FRAME_HEADER_SIZE, 80);
    }

    #[test]
    fn service_frame_header_encode_decode_roundtrip() {
        let request_id = RequestId::new(0xAAAA, 1, 0xBBBB);
        let deadline = Deadline::new(1000, 5000).unwrap();
        let header = ServiceFrameHeader::request(request_id, deadline, 0xCCCC, 0xDDDD);

        let mut buf = [0u8; 80];
        header.encode(&mut buf).unwrap();

        // 验证 magic 和 version
        assert_eq!(&buf[0..4], &SERVICE_FRAME_MAGIC.to_le_bytes());
        assert_eq!(&buf[4..6], &SERVICE_FRAME_VERSION.to_le_bytes());
        // 请求帧 error_code 为 Ok(0)
        assert_eq!(&buf[6..8], &0u16.to_le_bytes());

        let decoded = ServiceFrameHeader::decode(&buf).unwrap();
        assert_eq!(decoded, header);
    }

    #[test]
    fn service_frame_header_rejects_bad_magic() {
        let mut buf = [0u8; 80];
        buf[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        buf[4..6].copy_from_slice(&SERVICE_FRAME_VERSION.to_le_bytes());
        let err = ServiceFrameHeader::decode(&buf).unwrap_err();
        assert!(err.to_string().contains("magic"));
    }

    #[test]
    fn service_frame_header_rejects_bad_version() {
        let mut buf = [0u8; 80];
        buf[0..4].copy_from_slice(&SERVICE_FRAME_MAGIC.to_le_bytes());
        buf[4..6].copy_from_slice(&99u16.to_le_bytes());
        let err = ServiceFrameHeader::decode(&buf).unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn service_frame_header_rejects_unknown_error_code() {
        let request_id = RequestId::new(0xAAAA, 1, 0xBBBB);
        let deadline = Deadline::new(1000, 5000).unwrap();
        let header = ServiceFrameHeader::request(request_id, deadline, 0xCCCC, 0xDDDD);

        let mut buf = [0u8; 80];
        header.encode(&mut buf).unwrap();
        buf[6..8].copy_from_slice(&99u16.to_le_bytes());

        let err = ServiceFrameHeader::decode(&buf).unwrap_err();
        assert!(err.to_string().contains("error code"));
    }

    #[test]
    fn service_frame_header_rejects_zero_timeout() {
        let request_id = RequestId::new(0xAAAA, 1, 0xBBBB);
        let deadline = Deadline::new(1000, 5000).unwrap();
        let header = ServiceFrameHeader::request(request_id, deadline, 0xCCCC, 0xDDDD);

        let mut buf = [0u8; 80];
        header.encode(&mut buf).unwrap();
        buf[40..48].copy_from_slice(&0u64.to_le_bytes());

        let err = ServiceFrameHeader::decode(&buf).unwrap_err();
        assert!(err.to_string().contains("timeout_ms"));
    }

    #[test]
    fn service_frame_header_rejects_wrong_size() {
        let err = ServiceFrameHeader::decode(&[0u8; 79]).unwrap_err();
        assert_eq!(err.expected, 80);
        assert_eq!(err.actual, 79);
    }

    #[test]
    fn service_frame_roundtrip_request_with_payload() {
        let request_id = RequestId::new(100, 1, 0xABCD);
        let deadline = Deadline::new(2000, 500).unwrap();
        let header = ServiceFrameHeader::request(request_id, deadline, 0x9999, 0);

        let payload = b"hello, service!";
        let frame = encode_service_frame(&header, payload, b"").unwrap();

        assert!(frame.len() > SERVICE_FRAME_HEADER_SIZE);

        let (decoded_header, decoded_payload, decoded_error_msg) =
            decode_service_frame(&frame).unwrap();

        assert_eq!(decoded_header.magic, SERVICE_FRAME_MAGIC);
        assert_eq!(decoded_header.version, SERVICE_FRAME_VERSION);
        assert_eq!(decoded_header.error_code, 0);
        assert_eq!(decoded_header.session_id, 100);
        assert_eq!(decoded_header.sequence, 1);
        assert_eq!(decoded_header.service_id, 0xABCD);
        assert_eq!(decoded_header.correlation_id, 0x9999);
        assert_eq!(decoded_header.timeout_ms, 2000);
        assert_eq!(decoded_header.absolute_deadline_ms, 2500);
        assert_eq!(decoded_payload, payload);
        assert!(decoded_error_msg.is_empty());
    }

    #[test]
    fn service_frame_roundtrip_response_with_error() {
        let request_id = RequestId::new(200, 5, 0x1234);
        let deadline = Deadline::new(3000, 1000).unwrap();
        let header =
            ServiceFrameHeader::response(request_id, deadline, 0, 0, ServiceError::HandlerError);

        let error_msg = b"division by zero";
        let frame = encode_service_frame(&header, b"", error_msg).unwrap();

        let (decoded_header, decoded_payload, decoded_error_msg) =
            decode_service_frame(&frame).unwrap();

        assert_eq!(decoded_header.error_code, ServiceError::HandlerError as u16);
        assert!(decoded_payload.is_empty());
        assert_eq!(decoded_error_msg, error_msg);
    }

    #[test]
    fn service_frame_roundtrip_empty_payload_and_error() {
        let request_id = RequestId::new(1, 1, 1);
        let deadline = Deadline::new(100, 0).unwrap();
        let header = ServiceFrameHeader::request(request_id, deadline, 0, 0);

        let frame = encode_service_frame(&header, b"", b"").unwrap();
        let (decoded_header, payload, error_msg) = decode_service_frame(&frame).unwrap();

        assert_eq!(decoded_header, {
            let mut h = header;
            h.payload_span = VarSpan::default();
            h.error_msg_span = VarSpan::default();
            h
        });
        assert!(payload.is_empty());
        assert!(error_msg.is_empty());
    }

    #[test]
    fn service_frame_rejects_truncated_frame() {
        let err = decode_service_frame(&[0u8; 10]).unwrap_err();
        assert_eq!(err.expected, 80);
        assert_eq!(err.actual, 10);
    }

    #[test]
    fn service_frame_rejects_trailing_bytes() {
        let request_id = RequestId::new(1, 1, 1);
        let deadline = Deadline::new(100, 0).unwrap();
        let header = ServiceFrameHeader::request(request_id, deadline, 0, 0);
        let mut frame = encode_service_frame(&header, b"ok", b"").unwrap();
        frame.push(0xFF); // 追加非法尾部字节

        let err = decode_service_frame(&frame).unwrap_err();
        assert!(err.to_string().contains("trailing"));
    }

    #[test]
    fn fnv1a64_service_name_consistency() {
        // 不同名称得到不同 hash（无碰撞测试）
        let a = fnv1a64(b"service_a");
        let b = fnv1a64(b"service_b");
        assert_ne!(a, b);
    }
}
