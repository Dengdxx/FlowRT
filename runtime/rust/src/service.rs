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
/// 使用 transport 约定的绝对毫秒表示截止时间。ABI 中同时携带 timeout_ms（相对超时）
/// 和 absolute_deadline_ms（绝对截止），由调用方在发送时计算绝对值。inproc 可使用本地
/// monotonic clock；跨进程 transport 必须使用双方可比较的 clock domain。
///
/// 默认不允许无界等待：timeout_ms 为 0 表示非法值，解码时应报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    /// 相对超时毫秒数。0 表示非法（不允许无界等待）。
    pub timeout_ms: u64,
    /// 绝对截止时间毫秒，clock domain 由 transport 约定。
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
/// 48      8     absolute_deadline_ms (u64, transport 约定 clock domain 下的绝对截止)
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

    // ---- Inproc Service Runtime 测试 ----

    use crate::executor::{LaneId, ScheduleEvent, ScheduleWaiter};
    use crate::shutdown::ShutdownToken;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    #[test]
    fn inproc_service_normal_request_response() {
        let registry = ServiceRegistry::new();
        let (client, server) =
            registry.register("echo", LaneId(1), 8, |req: u32| req.wrapping_mul(2));

        // client 发送请求，server 在另一个线程处理
        let server_clone = server.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            server_clone.process_pending_requests();
        });

        let result = client.call(21, Duration::from_secs(1));
        handle.join().unwrap();

        assert!(result.is_ok());
        assert_eq!(result.ok_value(), Some(42));

        assert!(registry.has_service("echo"));
        assert_eq!(registry.service_count(), 1);
    }

    #[test]
    fn inproc_service_timeout_returns_timeout_error() {
        let registry = ServiceRegistry::new();
        let (client, _server) = registry.register("slow", LaneId(1), 8, |req: u32| {
            std::thread::sleep(Duration::from_millis(200));
            req
        });

        // 不启动 server 处理线程，client 直接超时
        let result = client.call(42, Duration::from_millis(10));
        assert!(!result.is_ok());
        assert_eq!(result.error_code(), ServiceError::Timeout);
    }

    #[test]
    fn inproc_service_unavailable_via_registry_check() {
        let registry = ServiceRegistry::new();
        assert!(!registry.has_service("nonexistent"));
        assert_eq!(registry.service_count(), 0);
    }

    #[test]
    fn inproc_service_unavailable_after_registry_drop() {
        // client 持有 Weak 引用，registry drop 后 call() 返回 Unavailable
        let registry = ServiceRegistry::new();
        let (client, _server) = registry.register("ephemeral", LaneId(1), 8, |req: u32| req);

        // registry 存活时调用正常（需要 server 处理，这里只测 Unavailable 路径）
        drop(registry);
        drop(_server);

        // registry 已 drop，client 的 Weak 升级失败 → Unavailable
        let result = client.call(42, Duration::from_secs(1));
        assert!(!result.is_ok());
        assert_eq!(result.error_code(), ServiceError::Unavailable);

        // start_call 同样返回 Unavailable
        let handle = client.start_call(42, Duration::from_secs(1));
        assert_eq!(handle.error, Some(ServiceError::Unavailable));
        assert!(handle.poll()); // error handle 立即可 poll
    }

    #[test]
    fn inproc_service_queue_depth_zero_rejects_all() {
        let registry = ServiceRegistry::new();
        let (client, _server) = registry.register("no_queue", LaneId(1), 0, |req: u32| req);

        // queue_depth=0 表示不允许排队，任何请求都返回 Busy
        let result = client.call(1, Duration::from_secs(1));
        assert!(!result.is_ok());
        assert_eq!(result.error_code(), ServiceError::Busy);
    }

    #[test]
    fn inproc_service_queue_full_returns_busy() {
        let registry = ServiceRegistry::new();
        // queue_depth = 1，允许 1 个排队请求
        let (client, _server) = registry.register("busy", LaneId(1), 1, |req: u32| req);

        // 第一个请求占满队列
        let h1 = client.start_call(1, Duration::from_secs(5));
        assert!(h1.error.is_none());

        // 第二个请求应该返回 Busy
        let h2 = client.start_call(2, Duration::from_secs(5));
        assert_eq!(h2.error, Some(ServiceError::Busy));
    }

    #[test]
    fn inproc_service_max_in_flight_limits_outstanding_calls() {
        let registry = ServiceRegistry::new();
        let config = InprocServiceConfig {
            queue_depth: 8,
            max_in_flight: 1,
            ..InprocServiceConfig::default()
        };
        let (client, server) =
            registry.register_with_config("limited", LaneId(1), config, |req: u32| req + 1);

        let h1 = client.start_call(1, Duration::from_secs(1));
        assert!(h1.error.is_none());

        let h2 = client.start_call(2, Duration::from_secs(1));
        assert_eq!(h2.error, Some(ServiceError::Busy));

        server.process_pending_requests();
        assert_eq!(h1.complete().ok_value(), Some(2));

        let h3 = client.start_call(3, Duration::from_secs(1));
        server.process_pending_requests();
        assert_eq!(h3.complete().ok_value(), Some(4));
    }

    #[test]
    fn inproc_service_overflow_error_policy_returns_rejected() {
        let registry = ServiceRegistry::new();
        let config = InprocServiceConfig {
            queue_depth: 1,
            overflow: ServiceOverflowPolicy::Error,
            ..InprocServiceConfig::default()
        };
        let (client, _server) =
            registry.register_with_config("overflow_error", LaneId(1), config, |req: u32| req);

        let h1 = client.start_call(1, Duration::from_secs(1));
        assert!(h1.error.is_none());

        let h2 = client.start_call(2, Duration::from_secs(1));
        assert_eq!(h2.error, Some(ServiceError::Rejected));
    }

    #[test]
    fn inproc_service_result_handler_can_return_service_error() {
        let registry = ServiceRegistry::new();
        let (client, server) =
            registry.register_result("fallible", LaneId(1), 8, |req: u32| -> ServiceResult<u32> {
                if req == 0 {
                    ServiceResult::err_with_message(ServiceError::Rejected, "zero")
                } else {
                    ServiceResult::ok(req * 2)
                }
            });

        let rejected = client.start_call(0, Duration::from_secs(1));
        server.process_pending_requests();
        let result = rejected.complete();
        assert_eq!(result.error_code(), ServiceError::Rejected);
        assert_eq!(result.error_message(), Some("zero"));

        let ok = client.start_call(3, Duration::from_secs(1));
        server.process_pending_requests();
        assert_eq!(ok.complete().ok_value(), Some(6));
    }

    #[test]
    fn inproc_service_request_arrival_wakes_server_waiter() {
        let registry = ServiceRegistry::new();
        let waiter = ScheduleWaiter::new();
        let config = InprocServiceConfig {
            schedule_waiter: waiter.clone(),
            ..InprocServiceConfig::default()
        };
        let (client, _server) =
            registry.register_with_config("wake_service", LaneId(1), config, |req: u32| req);

        let seen = waiter.data_generation();
        let handle = client.start_call(1, Duration::from_secs(1));
        assert!(handle.error.is_none());

        let event = waiter.wait_until_after(
            seen,
            Some(Instant::now() + Duration::from_millis(50)),
            &ShutdownToken::default(),
        );
        assert_eq!(event, ScheduleEvent::Data);
    }

    #[test]
    fn inproc_service_late_response_does_not_pollute_next_request() {
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);

        let registry = ServiceRegistry::new();
        let (client, server) = registry.register("counter", LaneId(2), 8, move |req: u32| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            req + 100
        });

        // 第一次调用：server 在另一个线程处理
        let server1 = server.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            server1.process_pending_requests();
        });
        let result1 = client.call(1, Duration::from_secs(1));
        handle.join().unwrap();
        assert_eq!(result1.ok_value(), Some(101));

        // 第二次调用：server 在另一个线程处理
        let server2 = server.clone();
        let handle2 = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            server2.process_pending_requests();
        });
        let result2 = client.call(2, Duration::from_secs(1));
        handle2.join().unwrap();
        assert_eq!(result2.ok_value(), Some(102));

        // handler 被调用了 2 次
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn inproc_service_late_response_is_dropped_after_handle_timeout() {
        let registry = ServiceRegistry::new();
        let (client, server) = registry.register("late_drop", LaneId(2), 8, |req: u32| req + 10);

        let handle = client.start_call(1, Duration::from_millis(1));
        assert_eq!(handle.complete().error_code(), ServiceError::Timeout);

        server.process_pending_requests();

        let stats = server.stats();
        assert_eq!(stats.timeout, 1);
        assert_eq!(stats.late_dropped, 1);
        assert_eq!(stats.success, 0);
    }

    #[test]
    fn inproc_service_same_lane_blocking_call_returns_would_deadlock() {
        let registry = ServiceRegistry::new();
        let (client, _server) = registry.register("deadlock", LaneId(5), 8, |req: u32| req);

        // 在 lane 5 上标记为活跃（模拟正在执行 task）
        let _guard = enter_lane(LaneId(5));

        // 同 lane 阻塞调用应该返回 WouldDeadlock
        let result = client.call(42, Duration::from_secs(1));
        assert!(!result.is_ok());
        assert_eq!(result.error_code(), ServiceError::WouldDeadlock);

        // 非阻塞调用也应该返回 WouldDeadlock
        let handle = client.start_call(42, Duration::from_secs(1));
        assert_eq!(handle.error, Some(ServiceError::WouldDeadlock));

        drop(_guard);

        // guard drop 后，跨 lane 调用应该正常
        let (client2, server2) =
            registry.register("ok_after_guard", LaneId(6), 8, |req: u32| req * 3);

        let handle_t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            server2.process_pending_requests();
        });
        let result = client2.call(10, Duration::from_secs(1));
        handle_t.join().unwrap();
        assert_eq!(result.ok_value(), Some(30));
    }

    #[test]
    fn inproc_service_non_blocking_handle_poll_and_complete() {
        let registry = ServiceRegistry::new();
        let (client, server) = registry.register("async_svc", LaneId(3), 8, |req: u32| req + 1);

        let handle = client.start_call(10, Duration::from_secs(1));
        assert!(!handle.poll());

        // 在另一个线程处理请求
        let handle_t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            server.process_pending_requests();
        });

        let result = handle.complete();
        handle_t.join().unwrap();

        assert!(result.is_ok());
        assert_eq!(result.ok_value(), Some(11));
    }

    #[test]
    fn inproc_service_server_stats_track_outcomes() {
        let registry = ServiceRegistry::new();
        let (client, server) = registry.register("stats_svc", LaneId(4), 8, |req: u32| req);

        // 正常调用
        let server_clone = server.clone();
        let handle_t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            server_clone.process_pending_requests();
        });
        let _ = client.call(1, Duration::from_secs(1));
        handle_t.join().unwrap();

        let stats = server.stats();
        assert_eq!(stats.requests, 1);
        assert_eq!(stats.success, 1);
        assert_eq!(stats.timeout, 0);
        assert_eq!(stats.busy, 0);
    }

    #[test]
    fn inproc_service_zero_timeout_returns_timeout() {
        let registry = ServiceRegistry::new();
        let (client, _server) = registry.register("zero_timeout", LaneId(1), 8, |req: u32| req);

        let result = client.call(42, Duration::ZERO);
        assert!(!result.is_ok());
        assert_eq!(result.error_code(), ServiceError::Timeout);
    }

    #[test]
    fn lane_guard_enter_and_exit() {
        // 进入 lane 前，ACTIVE_LANES 为空
        assert!(!ACTIVE_LANES.with(|lanes| lanes.borrow().contains(&LaneId(99))));

        {
            let _guard = enter_lane(LaneId(99));
            assert!(ACTIVE_LANES.with(|lanes| lanes.borrow().contains(&LaneId(99))));
        }

        // guard drop 后，lane 标记被移除
        assert!(!ACTIVE_LANES.with(|lanes| lanes.borrow().contains(&LaneId(99))));
    }

    #[test]
    fn service_registry_tracks_multiple_services() {
        let registry = ServiceRegistry::new();
        let (_c1, _s1) = registry.register("svc_a", LaneId(1), 4, |x: u32| x);
        let (_c2, _s2) = registry.register("svc_b", LaneId(2), 4, |x: u32| x);

        assert!(registry.has_service("svc_a"));
        assert!(registry.has_service("svc_b"));
        assert!(!registry.has_service("svc_c"));
        assert_eq!(registry.service_count(), 2);
    }
}

// ---------------------------------------------------------------------------
// Inproc Service Runtime
// ---------------------------------------------------------------------------
// 以下实现 inproc service 的 request/response 运行时：registry、typed client/server、
// 有界请求队列、same-lane 死锁检测、request arrival 唤醒和 late response 丢弃。
//
// 设计要点：
// - server 由 request arrival 驱动，不靠 tick polling
// - server 默认运行在 instance serial lane，保护组件线程安全
// - 默认 timeout 必须存在；不允许无界阻塞
// - callback 内无界 blocking 禁止；same-lane deadlock 返回 WouldDeadlock
// - queue 满默认返回 Busy
// - late response 不污染下一次 request

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::executor::{LaneId, ScheduleWaiter};

/// inproc service 队列溢出策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ServiceOverflowPolicy {
    /// 队列或 in-flight 上限满时返回 `Busy`。
    #[default]
    Busy,
    /// 队列或 in-flight 上限满时返回硬错误 `Rejected`。
    Error,
}

impl ServiceOverflowPolicy {
    fn error_code(self) -> ServiceError {
        match self {
            Self::Busy => ServiceError::Busy,
            Self::Error => ServiceError::Rejected,
        }
    }
}

/// inproc service 运行时配置。
#[derive(Debug, Clone)]
pub struct InprocServiceConfig {
    /// pending request 队列深度。0 表示不允许排队。
    pub queue_depth: usize,
    /// queued + executing request 总上限。0 表示不允许新请求。
    pub max_in_flight: usize,
    /// 队列或 in-flight 上限满时的错误映射。
    pub overflow: ServiceOverflowPolicy,
    /// request arrival 唤醒的 server scheduler waiter。
    pub schedule_waiter: ScheduleWaiter,
}

impl Default for InprocServiceConfig {
    fn default() -> Self {
        Self {
            queue_depth: 32,
            max_in_flight: 64,
            overflow: ServiceOverflowPolicy::Busy,
            schedule_waiter: ScheduleWaiter::new(),
        }
    }
}

/// inproc service 统计计数。
#[derive(Debug, Default)]
struct ServiceStats {
    requests: AtomicU64,
    success: AtomicU64,
    timeout: AtomicU64,
    busy: AtomicU64,
    late_dropped: AtomicU64,
    unavailable: AtomicU64,
    would_deadlock: AtomicU64,
    handler_error: AtomicU64,
}

impl ServiceStats {
    fn record_request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }
    fn record_success(&self) {
        self.success.fetch_add(1, Ordering::Relaxed);
    }
    fn record_timeout(&self) {
        self.timeout.fetch_add(1, Ordering::Relaxed);
    }
    fn record_busy(&self) {
        self.busy.fetch_add(1, Ordering::Relaxed);
    }
    fn record_late_dropped(&self) {
        self.late_dropped.fetch_add(1, Ordering::Relaxed);
    }
    // Unavailable 场景下 endpoint 已被 drop，stats 不可达，该方法预留供未来扩展。
    #[allow(dead_code)]
    fn record_unavailable(&self) {
        self.unavailable.fetch_add(1, Ordering::Relaxed);
    }
    fn record_would_deadlock(&self) {
        self.would_deadlock.fetch_add(1, Ordering::Relaxed);
    }
    #[allow(dead_code)]
    fn record_handler_error(&self) {
        self.handler_error.fetch_add(1, Ordering::Relaxed);
    }

    /// 返回 (requests, success, timeout, busy, late_dropped, unavailable, would_deadlock, handler_error)。
    fn snapshot(&self) -> (u64, u64, u64, u64, u64, u64, u64, u64) {
        (
            self.requests.load(Ordering::Relaxed),
            self.success.load(Ordering::Relaxed),
            self.timeout.load(Ordering::Relaxed),
            self.busy.load(Ordering::Relaxed),
            self.late_dropped.load(Ordering::Relaxed),
            self.unavailable.load(Ordering::Relaxed),
            self.would_deadlock.load(Ordering::Relaxed),
            self.handler_error.load(Ordering::Relaxed),
        )
    }
}

fn record_service_result_stats<T>(stats: &ServiceStats, result: &ServiceResult<T>) {
    if result.is_ok() {
        stats.record_success();
    } else if result.error_code() == ServiceError::HandlerError {
        stats.record_handler_error();
    }
}

/// type-erased request closure，携带 typed response slot。
type ErasedRequest = Box<dyn FnOnce() + Send + 'static>;

/// type-erased pending request 队列。
type ErasedQueue = Arc<Mutex<VecDeque<ErasedRequest>>>;

/// inproc service 响应槽。
///
/// client 创建 response slot，通过 request closure 传递给 server handler。
/// handler 执行完毕后将响应写入 slot 并唤醒等待方。
struct ResponseSlot<T> {
    ready: AtomicBool,
    abandoned: AtomicBool,
    value: Mutex<Option<ServiceResult<T>>>,
    condvar: Condvar,
}

impl<T> ResponseSlot<T> {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            ready: AtomicBool::new(false),
            abandoned: AtomicBool::new(false),
            value: Mutex::new(None),
            condvar: Condvar::new(),
        })
    }

    /// server 端写入响应并唤醒 client。
    fn fill(&self, value: ServiceResult<T>) -> bool {
        if self.abandoned.load(Ordering::Acquire) {
            return false;
        }
        {
            let mut guard = self.value.lock().unwrap_or_else(|e| e.into_inner());
            if self.abandoned.load(Ordering::Acquire) {
                return false;
            }
            *guard = Some(value);
            self.ready.store(true, Ordering::Release);
        }
        self.condvar.notify_one();
        true
    }

    /// client 端轮询是否已就绪。
    fn poll(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// client 端阻塞等待响应，直到 deadline 或超时。
    fn wait_until(&self, deadline: Instant) -> Option<ServiceResult<T>> {
        let mut guard = self.value.lock().unwrap_or_else(|e| e.into_inner());
        while !self.ready.load(Ordering::Acquire) {
            let now = Instant::now();
            if now >= deadline {
                self.abandon();
                return None;
            }
            let timeout = deadline.duration_since(now);
            let (next, result) = match self.condvar.wait_timeout(guard, timeout) {
                Ok(pair) => pair,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard = next;
            if result.timed_out() && !self.ready.load(Ordering::Acquire) {
                self.abandon();
                return None;
            }
        }
        guard.take()
    }

    fn abandon(&self) {
        self.abandoned.store(true, Ordering::Release);
    }
}

/// type-erased service endpoint trait，供 `ServiceRegistry` 统一存储。
#[allow(dead_code)]
trait ErasedEndpoint: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn process_pending(&self);
    fn is_available(&self) -> bool;
}

/// 共享 endpoint 内部状态，client 和 server handle 共用。
struct InprocEndpointInner<Req: Send + 'static, Resp: Send + 'static> {
    queue: ErasedQueue,
    handler: Arc<dyn Fn(Req) -> ServiceResult<Resp> + Send + Sync>,
    lane: LaneId,
    stats: Arc<ServiceStats>,
    queue_depth: usize,
    max_in_flight: usize,
    in_flight: AtomicUsize,
    overflow: ServiceOverflowPolicy,
    schedule_waiter: ScheduleWaiter,
}

/// inproc service endpoint，承载 typed request/response 队列和 handler。
///
/// client 和 server handle 共享同一个 endpoint 的 Arc。server handle 调用
/// `process_pending_requests()` 驱动 handler 执行。
struct InprocEndpoint<Req: Send + 'static, Resp: Send + 'static> {
    inner: Arc<InprocEndpointInner<Req, Resp>>,
}

impl<Req: Send + 'static, Resp: Send + 'static> InprocEndpoint<Req, Resp> {
    fn new(
        queue: ErasedQueue,
        lane: LaneId,
        config: InprocServiceConfig,
        handler: impl Fn(Req) -> ServiceResult<Resp> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(InprocEndpointInner {
                queue,
                handler: Arc::new(handler),
                lane,
                stats: Arc::new(ServiceStats::default()),
                queue_depth: config.queue_depth,
                max_in_flight: config.max_in_flight,
                in_flight: AtomicUsize::new(0),
                overflow: config.overflow,
                schedule_waiter: config.schedule_waiter,
            }),
        })
    }
}

impl<Req: Send + 'static, Resp: Send + 'static> ErasedEndpoint for InprocEndpoint<Req, Resp> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn process_pending(&self) {
        let pending: Vec<ErasedRequest> = {
            let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.drain(..).collect()
        };
        for req in pending {
            req();
        }
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// 服务统计快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatsSnapshot {
    /// 总请求数。
    pub requests: u64,
    /// 成功响应数。
    pub success: u64,
    /// 超时数。
    pub timeout: u64,
    /// 队列满拒绝数。
    pub busy: u64,
    /// client 超时或取消后到达并被丢弃的 late response 数。
    pub late_dropped: u64,
    /// 服务不可用数。
    pub unavailable: u64,
    /// 死锁检测拒绝数。
    pub would_deadlock: u64,
    /// handler 错误数。
    pub handler_error: u64,
}

/// inproc service 注册中心。
///
/// 管理所有注册的 service endpoint，提供 typed 注册和 type-erased 查询。
/// server 由 request arrival 驱动，不靠 tick polling。
pub struct ServiceRegistry {
    endpoints: Mutex<HashMap<String, Arc<dyn ErasedEndpoint>>>,
}

impl ServiceRegistry {
    /// 构造空注册中心。
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            endpoints: Mutex::new(HashMap::new()),
        })
    }

    /// 注册 typed inproc service。
    ///
    /// 返回 `(client, server)` handle 对。client 可以 `call()` / `start_call()`，
    /// server 需要在 scheduler 中调用 `process_pending_requests()`。
    ///
    /// - `name`：service 唯一名称。
    /// - `lane`：server 所在 lane id，用于 same-lane 死锁检测。
    /// - `queue_depth`：请求队列深度，0 表示不允许排队。
    /// - `handler`：请求处理函数，同步执行。
    pub fn register<Req, Resp, F>(
        self: &Arc<Self>,
        name: &str,
        lane: LaneId,
        queue_depth: usize,
        handler: F,
    ) -> (
        InprocServiceClient<Req, Resp>,
        InprocServiceServer<Req, Resp>,
    )
    where
        Req: Send + 'static,
        Resp: Send + 'static,
        F: Fn(Req) -> Resp + Send + Sync + 'static,
    {
        self.register_result_with_config(
            name,
            lane,
            InprocServiceConfig {
                queue_depth,
                ..InprocServiceConfig::default()
            },
            move |req| ServiceResult::ok(handler(req)),
        )
    }

    /// 注册返回 `ServiceResult` 的 typed inproc service。
    pub fn register_result<Req, Resp, F>(
        self: &Arc<Self>,
        name: &str,
        lane: LaneId,
        queue_depth: usize,
        handler: F,
    ) -> (
        InprocServiceClient<Req, Resp>,
        InprocServiceServer<Req, Resp>,
    )
    where
        Req: Send + 'static,
        Resp: Send + 'static,
        F: Fn(Req) -> ServiceResult<Resp> + Send + Sync + 'static,
    {
        self.register_result_with_config(
            name,
            lane,
            InprocServiceConfig {
                queue_depth,
                ..InprocServiceConfig::default()
            },
            handler,
        )
    }

    /// 使用显式配置注册 infallible typed inproc service。
    pub fn register_with_config<Req, Resp, F>(
        self: &Arc<Self>,
        name: &str,
        lane: LaneId,
        config: InprocServiceConfig,
        handler: F,
    ) -> (
        InprocServiceClient<Req, Resp>,
        InprocServiceServer<Req, Resp>,
    )
    where
        Req: Send + 'static,
        Resp: Send + 'static,
        F: Fn(Req) -> Resp + Send + Sync + 'static,
    {
        self.register_result_with_config(name, lane, config, move |req| {
            ServiceResult::ok(handler(req))
        })
    }

    /// 使用显式配置注册返回 `ServiceResult` 的 typed inproc service。
    pub fn register_result_with_config<Req, Resp, F>(
        self: &Arc<Self>,
        name: &str,
        lane: LaneId,
        config: InprocServiceConfig,
        handler: F,
    ) -> (
        InprocServiceClient<Req, Resp>,
        InprocServiceServer<Req, Resp>,
    )
    where
        Req: Send + 'static,
        Resp: Send + 'static,
        F: Fn(Req) -> ServiceResult<Resp> + Send + Sync + 'static,
    {
        let queue: ErasedQueue = Arc::new(Mutex::new(VecDeque::new()));
        let endpoint = InprocEndpoint::new(Arc::clone(&queue), lane, config, handler);

        {
            let mut endpoints = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
            let erased: Arc<dyn ErasedEndpoint> = endpoint.clone();
            endpoints.insert(name.to_string(), erased);
        }

        let client = InprocServiceClient {
            endpoint: Arc::downgrade(&endpoint.inner),
            name: name.to_string(),
        };
        let server = InprocServiceServer {
            endpoint,
            name: name.to_string(),
        };
        (client, server)
    }

    /// 查询 service 是否已注册。
    pub fn has_service(&self, name: &str) -> bool {
        let endpoints = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
        endpoints.contains_key(name)
    }

    /// 返回已注册 service 数量。
    pub fn service_count(&self) -> usize {
        let endpoints = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
        endpoints.len()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self {
            endpoints: Mutex::new(HashMap::new()),
        }
    }
}

/// inproc service client handle。
///
/// 支持阻塞 `call()` 和非阻塞 `start_call()`。same-lane 调用会被检测并返回
/// `WouldDeadlock`，避免同步执行模型下的确定性死锁。
///
/// client 通过 `Weak` 引用 endpoint：当 `ServiceRegistry` 被 drop 后，client 的
/// `call()` / `start_call()` 会返回 `Unavailable`，满足 "server 未注册返回 Unavailable"
/// 的运行时语义。
pub struct InprocServiceClient<Req: Send + 'static, Resp: Send + 'static> {
    endpoint: std::sync::Weak<InprocEndpointInner<Req, Resp>>,
    name: String,
}

impl<Req: Send + 'static, Resp: Send + 'static> Clone for InprocServiceClient<Req, Resp> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            name: self.name.clone(),
        }
    }
}

impl<Req: Send + 'static, Resp: Send + 'static> InprocServiceClient<Req, Resp> {
    /// 尝试升级 endpoint 引用。endpoint 已被 drop 时返回 `Unavailable`。
    fn upgrade(&self) -> Result<Arc<InprocEndpointInner<Req, Resp>>, ServiceError> {
        self.endpoint.upgrade().ok_or(ServiceError::Unavailable)
    }

    /// 阻塞调用，deadline-bound。
    ///
    /// 如果 service 已不可用（registry 被 drop），返回 `Unavailable`。
    /// 如果 server 所在 lane 与当前活跃 lane 相同，立即返回 `WouldDeadlock`。
    /// 如果请求队列已满，返回 `Busy`。
    /// 如果超时，返回 `Timeout`。
    pub fn call(&self, request: Req, timeout: Duration) -> ServiceResult<Resp> {
        let inner = match self.upgrade() {
            Ok(inner) => inner,
            Err(e) => return ServiceResult::err(e),
        };
        inner.stats.record_request();

        if timeout.is_zero() {
            inner.stats.record_timeout();
            return ServiceResult::err(ServiceError::Timeout);
        }

        if ACTIVE_LANES.with(|lanes| {
            let lanes = lanes.borrow();
            lanes.contains(&inner.lane)
        }) {
            inner.stats.record_would_deadlock();
            return ServiceResult::err(ServiceError::WouldDeadlock);
        }

        let response_slot = ResponseSlot::new();
        let slot_for_closure = Arc::clone(&response_slot);
        let handler = Arc::clone(&inner.handler);
        let stats = Arc::clone(&inner.stats);
        let inner_for_closure = Arc::clone(&inner);

        let enqueue_result = {
            let mut queue = inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            if queue.len() >= inner.queue_depth
                || inner.in_flight.load(Ordering::Acquire) >= inner.max_in_flight
            {
                Err(inner.overflow.error_code())
            } else {
                inner.in_flight.fetch_add(1, Ordering::AcqRel);
                queue.push_back(Box::new(move || {
                    let result = handler(request);
                    if !slot_for_closure.fill(result) {
                        stats.record_late_dropped();
                    }
                    inner_for_closure.in_flight.fetch_sub(1, Ordering::AcqRel);
                }));
                Ok(())
            }
        };

        if let Err(error) = enqueue_result {
            inner.stats.record_busy();
            return ServiceResult::err(error);
        }

        inner.schedule_waiter.notify_data();

        let deadline = Instant::now() + timeout;
        match response_slot.wait_until(deadline) {
            Some(result) => {
                record_service_result_stats(&inner.stats, &result);
                result
            }
            None => {
                inner.stats.record_timeout();
                ServiceResult::err(ServiceError::Timeout)
            }
        }
    }

    /// 非阻塞调用，返回 handle 供后续 poll/complete。
    ///
    /// same-lane 和 queue 满检测与 `call()` 一致。
    pub fn start_call(&self, request: Req, timeout: Duration) -> ServiceCallHandle<Resp> {
        let inner = match self.upgrade() {
            Ok(inner) => inner,
            Err(e) => {
                return ServiceCallHandle {
                    slot: ResponseSlot::new(),
                    deadline: Instant::now(),
                    error: Some(e),
                    stats: None,
                };
            }
        };
        inner.stats.record_request();

        if timeout.is_zero() {
            inner.stats.record_timeout();
            return ServiceCallHandle {
                slot: ResponseSlot::new(),
                deadline: Instant::now(),
                error: Some(ServiceError::Timeout),
                stats: None,
            };
        }

        if ACTIVE_LANES.with(|lanes| {
            let lanes = lanes.borrow();
            lanes.contains(&inner.lane)
        }) {
            inner.stats.record_would_deadlock();
            return ServiceCallHandle {
                slot: ResponseSlot::new(),
                deadline: Instant::now(),
                error: Some(ServiceError::WouldDeadlock),
                stats: None,
            };
        }

        let response_slot = ResponseSlot::new();
        let slot_for_closure = Arc::clone(&response_slot);
        let handler = Arc::clone(&inner.handler);
        let stats = Arc::clone(&inner.stats);
        let stats_for_closure = Arc::clone(&stats);
        let inner_for_closure = Arc::clone(&inner);

        let enqueue_result = {
            let mut queue = inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            if queue.len() >= inner.queue_depth
                || inner.in_flight.load(Ordering::Acquire) >= inner.max_in_flight
            {
                Err(inner.overflow.error_code())
            } else {
                inner.in_flight.fetch_add(1, Ordering::AcqRel);
                queue.push_back(Box::new(move || {
                    let result = handler(request);
                    if !slot_for_closure.fill(result) {
                        stats_for_closure.record_late_dropped();
                    }
                    inner_for_closure.in_flight.fetch_sub(1, Ordering::AcqRel);
                }));
                Ok(())
            }
        };

        if let Err(error) = enqueue_result {
            inner.stats.record_busy();
            return ServiceCallHandle {
                slot: ResponseSlot::new(),
                deadline: Instant::now(),
                error: Some(error),
                stats: None,
            };
        }

        inner.schedule_waiter.notify_data();

        ServiceCallHandle {
            slot: response_slot,
            deadline: Instant::now() + timeout,
            error: None,
            stats: Some(stats),
        }
    }

    /// 返回 service 名称。
    pub fn service_name(&self) -> &str {
        &self.name
    }

    /// 返回 server 所在 lane。
    pub fn server_lane(&self) -> LaneId {
        self.endpoint
            .upgrade()
            .map(|inner| inner.lane)
            .unwrap_or(LaneId(0))
    }
}

/// 非阻塞调用句柄。
///
/// client 通过 `start_call()` 获取，后续通过 `poll()` 检查就绪状态或 `complete()`
/// 阻塞等待结果。
pub struct ServiceCallHandle<Resp: Send + 'static> {
    slot: Arc<ResponseSlot<Resp>>,
    deadline: Instant,
    error: Option<ServiceError>,
    stats: Option<Arc<ServiceStats>>,
}

impl<Resp: Send + 'static> ServiceCallHandle<Resp> {
    /// 轮询响应是否已就绪。
    pub fn poll(&self) -> bool {
        if self.error.is_some() {
            return true;
        }
        self.slot.poll()
    }

    /// 阻塞等待响应完成，消耗 handle。
    pub fn complete(self) -> ServiceResult<Resp> {
        if let Some(err) = self.error {
            return ServiceResult::err(err);
        }

        match self.slot.wait_until(self.deadline) {
            Some(result) => {
                if let Some(stats) = &self.stats {
                    record_service_result_stats(stats, &result);
                }
                result
            }
            None => {
                if let Some(stats) = &self.stats {
                    stats.record_timeout();
                }
                ServiceResult::err(ServiceError::Timeout)
            }
        }
    }
}

thread_local! {
    /// 当前线程活跃的 lane 集合，用于 same-lane 死锁检测。
    static ACTIVE_LANES: std::cell::RefCell<Vec<LaneId>> = const { std::cell::RefCell::new(Vec::new()) };

}

/// 在当前线程标记 lane 为活跃，返回 RAII guard。
///
/// guard drop 时自动移除标记。用于 generated shell 在执行 task 期间标记 lane，
/// 使 same-lane service call 能被检测为 `WouldDeadlock`。
pub fn enter_lane(lane: LaneId) -> LaneGuard {
    ACTIVE_LANES.with(|lanes| {
        lanes.borrow_mut().push(lane);
    });
    LaneGuard { lane }
}

/// lane 活跃标记的 RAII guard。
pub struct LaneGuard {
    lane: LaneId,
}

impl Drop for LaneGuard {
    fn drop(&mut self) {
        ACTIVE_LANES.with(|lanes| {
            let mut lanes = lanes.borrow_mut();
            if let Some(pos) = lanes.iter().rposition(|l| *l == self.lane) {
                lanes.remove(pos);
            }
        });
    }
}

/// inproc service server handle。
///
/// server 由 request arrival 驱动。generated shell 在 scheduler 中调用
/// `process_pending_requests()` 处理所有排队请求。
pub struct InprocServiceServer<Req: Send + 'static, Resp: Send + 'static> {
    endpoint: Arc<InprocEndpoint<Req, Resp>>,
    name: String,
}

impl<Req: Send + 'static, Resp: Send + 'static> Clone for InprocServiceServer<Req, Resp> {
    fn clone(&self) -> Self {
        Self {
            endpoint: Arc::clone(&self.endpoint),
            name: self.name.clone(),
        }
    }
}

impl<Req: Send + 'static, Resp: Send + 'static> InprocServiceServer<Req, Resp> {
    /// 处理所有排队的 pending request。
    ///
    /// 该方法 drain 整个队列后逐一调用 handler。handler 内部的嵌套 service 调用
    /// 会在下一次 `process_pending_requests()` 时被处理，避免无限递归。
    pub fn process_pending_requests(&self) {
        self.endpoint.process_pending();
    }

    /// 返回 service 名称。
    pub fn service_name(&self) -> &str {
        &self.name
    }

    /// 返回 server 所在 lane。
    pub fn lane(&self) -> LaneId {
        self.endpoint.inner.lane
    }

    /// 返回统计快照。
    pub fn stats(&self) -> ServiceStatsSnapshot {
        let (
            requests,
            success,
            timeout,
            busy,
            late_dropped,
            unavailable,
            would_deadlock,
            handler_error,
        ) = self.endpoint.inner.stats.snapshot();
        ServiceStatsSnapshot {
            requests,
            success,
            timeout,
            busy,
            late_dropped,
            unavailable,
            would_deadlock,
            handler_error,
        }
    }

    /// 返回 request arrival 唤醒的 scheduler waiter。
    pub fn request_waiter(&self) -> ScheduleWaiter {
        self.endpoint.inner.schedule_waiter.clone()
    }
}
