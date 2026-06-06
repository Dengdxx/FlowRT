//! Service frame roundtrip 和 ABI 稳定性测试。

use std::mem::{offset_of, size_of};

use flowrt::{
    Deadline, RequestId, ServiceError, ServiceFrameHeader, ServiceResult,
    SERVICE_FRAME_HEADER_SIZE, SERVICE_FRAME_MAGIC, SERVICE_FRAME_VERSION,
    abi::{
        FLOWRT_SERVICE_BACKEND, FLOWRT_SERVICE_BUSY, FLOWRT_SERVICE_CANCELLED,
        FLOWRT_SERVICE_DEADLINE_EXCEEDED, FLOWRT_SERVICE_HANDLER_ERROR, FLOWRT_SERVICE_OK,
        FLOWRT_SERVICE_PROTOCOL, FLOWRT_SERVICE_REJECTED, FLOWRT_SERVICE_TIMEOUT,
        FLOWRT_SERVICE_UNAVAILABLE, FLOWRT_SERVICE_WOULD_DEADLOCK,
        FLOWRT_SERVICE_FRAME_HEADER_SIZE, FLOWRT_SERVICE_FRAME_MAGIC,
        FLOWRT_SERVICE_FRAME_VERSION,
        FlowrtServiceFrameHeader, service_error_to_abi, service_frame_header_to_abi,
    },
    decode_service_frame, encode_service_frame, fnv1a64,
};

#[test]
fn service_error_abi_constants_are_stable() {
    assert_eq!(FLOWRT_SERVICE_OK, 0);
    assert_eq!(FLOWRT_SERVICE_TIMEOUT, 1);
    assert_eq!(FLOWRT_SERVICE_UNAVAILABLE, 2);
    assert_eq!(FLOWRT_SERVICE_BUSY, 3);
    assert_eq!(FLOWRT_SERVICE_REJECTED, 4);
    assert_eq!(FLOWRT_SERVICE_CANCELLED, 5);
    assert_eq!(FLOWRT_SERVICE_DEADLINE_EXCEEDED, 6);
    assert_eq!(FLOWRT_SERVICE_PROTOCOL, 7);
    assert_eq!(FLOWRT_SERVICE_BACKEND, 8);
    assert_eq!(FLOWRT_SERVICE_WOULD_DEADLOCK, 9);
    assert_eq!(FLOWRT_SERVICE_HANDLER_ERROR, 10);
}

#[test]
fn service_error_to_abi_conversion() {
    assert_eq!(service_error_to_abi(ServiceError::Ok), FLOWRT_SERVICE_OK);
    assert_eq!(service_error_to_abi(ServiceError::Timeout), FLOWRT_SERVICE_TIMEOUT);
    assert_eq!(service_error_to_abi(ServiceError::HandlerError), FLOWRT_SERVICE_HANDLER_ERROR);
}

#[test]
fn service_frame_abi_constants_match_rust_constants() {
    assert_eq!(FLOWRT_SERVICE_FRAME_MAGIC, SERVICE_FRAME_MAGIC);
    assert_eq!(FLOWRT_SERVICE_FRAME_VERSION, SERVICE_FRAME_VERSION);
    assert_eq!(FLOWRT_SERVICE_FRAME_HEADER_SIZE, SERVICE_FRAME_HEADER_SIZE);
}

#[test]
fn service_frame_header_abi_struct_layout() {
    assert_eq!(size_of::<FlowrtServiceFrameHeader>(), 80);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, magic), 0);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, version), 4);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, error_code), 6);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, service_id), 8);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, session_id), 16);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, sequence), 24);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, correlation_id), 32);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, timeout_ms), 40);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, absolute_deadline_ms), 48);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, schema_hash), 56);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, payload_offset), 64);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, payload_len), 68);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, error_msg_offset), 72);
    assert_eq!(offset_of!(FlowrtServiceFrameHeader, error_msg_len), 76);
}

#[test]
fn service_frame_header_to_abi_preserves_fields() {
    let request_id = RequestId::new(0xAA, 1, 0xBB);
    let deadline = Deadline::new(500, 1000).unwrap();
    let header = ServiceFrameHeader::request(request_id, deadline, 0xCC, 0xDD);

    let abi = service_frame_header_to_abi(&header);
    assert_eq!(abi.magic, SERVICE_FRAME_MAGIC);
    assert_eq!(abi.version, SERVICE_FRAME_VERSION);
    assert_eq!(abi.error_code, 0);
    assert_eq!(abi.service_id, 0xBB);
    assert_eq!(abi.session_id, 0xAA);
    assert_eq!(abi.sequence, 1);
    assert_eq!(abi.correlation_id, 0xCC);
    assert_eq!(abi.timeout_ms, 500);
    assert_eq!(abi.absolute_deadline_ms, 1500);
    assert_eq!(abi.schema_hash, 0xDD);
}

#[test]
fn service_frame_roundtrip_request_with_payload_and_correlation() {
    let request_id = RequestId::new(42, 7, fnv1a64(b"my_service"));
    let deadline = Deadline::new(3000, 100).unwrap();
    let header = ServiceFrameHeader::request(request_id, deadline, 0xDEAD, 0xBEEF);

    let payload = b"request payload data";
    let frame = encode_service_frame(&header, payload, b"").unwrap();

    let (decoded, decoded_payload, decoded_error) = decode_service_frame(&frame).unwrap();

    assert_eq!(decoded.magic, SERVICE_FRAME_MAGIC);
    assert_eq!(decoded.version, SERVICE_FRAME_VERSION);
    assert_eq!(decoded.error_code, ServiceError::Ok as u16);
    assert_eq!(decoded.session_id, 42);
    assert_eq!(decoded.sequence, 7);
    assert_eq!(decoded.service_id, fnv1a64(b"my_service"));
    assert_eq!(decoded.correlation_id, 0xDEAD);
    assert_eq!(decoded.timeout_ms, 3000);
    assert_eq!(decoded.absolute_deadline_ms, 3100);
    assert_eq!(decoded.schema_hash, 0xBEEF);
    assert_eq!(decoded_payload, payload);
    assert!(decoded_error.is_empty());
}

#[test]
fn service_frame_roundtrip_error_response() {
    let request_id = RequestId::new(100, 5, fnv1a64(b"sensor"));
    let deadline = Deadline::new(1000, 500).unwrap();
    let header = ServiceFrameHeader::response(
        request_id, deadline, 0x99, 0,
        ServiceError::HandlerError,
    );

    let error_msg = b"division by zero";
    let frame = encode_service_frame(&header, b"", error_msg).unwrap();

    let (decoded, payload, decoded_error) = decode_service_frame(&frame).unwrap();

    assert_eq!(decoded.error_code, ServiceError::HandlerError as u16);
    assert!(payload.is_empty());
    assert_eq!(decoded_error, error_msg);
}

#[test]
fn service_frame_roundtrip_empty_frame() {
    let request_id = RequestId::new(1, 1, 1);
    let deadline = Deadline::new(100, 0).unwrap();
    let header = ServiceFrameHeader::request(request_id, deadline, 0, 0);

    let frame = encode_service_frame(&header, b"", b"").unwrap();
    let (decoded, payload, error_msg) = decode_service_frame(&frame).unwrap();

    assert_eq!(decoded.magic, SERVICE_FRAME_MAGIC);
    assert!(payload.is_empty());
    assert!(error_msg.is_empty());
}

#[test]
fn service_frame_rejects_wrong_magic() {
    let mut frame = vec![0u8; 80];
    frame[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    frame[4..6].copy_from_slice(&SERVICE_FRAME_VERSION.to_le_bytes());
    let err = decode_service_frame(&frame).unwrap_err();
    assert!(err.to_string().contains("magic"));
}

#[test]
fn service_frame_rejects_wrong_version() {
    let mut frame = vec![0u8; 80];
    frame[0..4].copy_from_slice(&SERVICE_FRAME_MAGIC.to_le_bytes());
    frame[4..6].copy_from_slice(&99u16.to_le_bytes());
    let err = decode_service_frame(&frame).unwrap_err();
    assert!(err.to_string().contains("version"));
}

#[test]
fn service_frame_rejects_truncated_header() {
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
    frame.push(0xFF);

    let err = decode_service_frame(&frame).unwrap_err();
    assert!(err.to_string().contains("trailing"));
}

#[test]
fn service_frame_size_is_80() {
    assert_eq!(SERVICE_FRAME_HEADER_SIZE, 80);
}

#[test]
fn service_frame_version_is_1() {
    assert_eq!(SERVICE_FRAME_VERSION, 1);
}

#[test]
fn fnv1a64_consistency() {
    assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
    assert_ne!(fnv1a64(b"service_a"), fnv1a64(b"service_b"));
}

#[test]
fn deadline_rejects_zero_timeout() {
    assert!(Deadline::new(0, 1000).is_none());
}

#[test]
fn deadline_computes_absolute_and_expiry() {
    let d = Deadline::new(500, 1000).unwrap();
    assert_eq!(d.timeout_ms, 500);
    assert_eq!(d.absolute_deadline_ms, 1500);
    assert!(!d.expired(1499));
    assert!(d.expired(1500));
}

#[test]
fn service_result_semantics() {
    let ok: ServiceResult<u32> = ServiceResult::ok(42);
    assert!(ok.is_ok());
    assert_eq!(ok.error_code(), ServiceError::Ok);
    assert_eq!(ok.ok_value(), Some(42));

    let err: ServiceResult<u32> = ServiceResult::err_with_message(
        ServiceError::Timeout,
        "timed out",
    );
    assert!(err.is_err());
    assert_eq!(err.error_code(), ServiceError::Timeout);
    assert_eq!(err.error_message(), Some("timed out"));
    assert!(err.ok_value().is_none());
}
