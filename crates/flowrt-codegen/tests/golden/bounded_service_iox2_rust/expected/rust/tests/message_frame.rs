// FlowRT 管理产物。不要手工修改。

use flowrt::FrameCodec;

fn corrupt_var_span(mut frame: Vec<u8>, header_offset: usize, offset: u32, len: u32) -> Vec<u8> {
    frame[header_offset..header_offset + 4].copy_from_slice(&offset.to_le_bytes());
    frame[header_offset + 4..header_offset + 8].copy_from_slice(&len.to_le_bytes());
    frame
}

const EXPECTED_PLAN_REQUEST_FRAME: &[u8] = &[3, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 6, 0, 0, 0, 8, 0, 0, 0, 117, 116, 102, 56, 45, 51, 5, 0, 0, 0, 6, 0, 0, 0];
const EXPECTED_PLAN_REQUEST_EMPTY_FRAME: &[u8] = &[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn sample_plan_request() -> flowrt_app::messages::PlanRequest {
    flowrt_app::messages::PlanRequest {
        goal: 3u32,
        label: "utf8-3".to_string(),
        samples: vec![5u32, 6u32],
    }
}

fn sample_plan_request_empty() -> flowrt_app::messages::PlanRequest {
    flowrt_app::messages::PlanRequest {
        goal: 3u32,
        label: String::new(),
        samples: Vec::new(),
    }
}

#[test]
fn plan_request_canonical_frame_codec() {
    let value = sample_plan_request();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PLAN_REQUEST_FRAME);
    assert_eq!(flowrt_app::messages::PlanRequest::decode_frame(&frame).unwrap(), value);
}

#[test]
fn plan_request_empty_variable_fields_frame_codec() {
    let value = sample_plan_request_empty();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PLAN_REQUEST_EMPTY_FRAME);
    assert_eq!(flowrt_app::messages::PlanRequest::decode_frame(&frame).unwrap(), value);
}

#[test]
fn plan_request_rejects_malformed_frame_decode() {
    let truncated = &EXPECTED_PLAN_REQUEST_FRAME[..19];
    assert!(flowrt_app::messages::PlanRequest::decode_frame(truncated).unwrap_err().to_string().contains("wire payload size mismatch"));
    let offset_overflow = corrupt_var_span(EXPECTED_PLAN_REQUEST_FRAME.to_vec(), 4, u32::MAX, 1);
    assert!(flowrt_app::messages::PlanRequest::decode_frame(&offset_overflow).unwrap_err().to_string().contains("variable tail blocks are not canonical"));
    let length_overflow = corrupt_var_span(EXPECTED_PLAN_REQUEST_FRAME.to_vec(), 4, 0, u32::MAX);
    assert!(flowrt_app::messages::PlanRequest::decode_frame(&length_overflow).unwrap_err().to_string().contains("variable span exceeds frame tail length"));
    let mut invalid_utf8 = EXPECTED_PLAN_REQUEST_FRAME.to_vec();
    invalid_utf8[20] = 0xff;
    assert!(flowrt_app::messages::PlanRequest::decode_frame(&invalid_utf8).unwrap_err().to_string().contains("string field is not valid UTF-8"));
}

const EXPECTED_PLAN_RESPONSE_FRAME: &[u8] = &[1, 0, 0, 0, 0, 9, 0, 0, 0, 117, 116, 102, 56, 45, 206, 188, 45, 51];
const EXPECTED_PLAN_RESPONSE_EMPTY_FRAME: &[u8] = &[1, 0, 0, 0, 0, 0, 0, 0, 0];

fn sample_plan_response() -> flowrt_app::messages::PlanResponse {
    flowrt_app::messages::PlanResponse {
        accepted: true,
        detail: "utf8-\u{03bc}-3".to_string(),
    }
}

fn sample_plan_response_empty() -> flowrt_app::messages::PlanResponse {
    flowrt_app::messages::PlanResponse {
        accepted: true,
        detail: String::new(),
    }
}

#[test]
fn plan_response_canonical_frame_codec() {
    let value = sample_plan_response();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PLAN_RESPONSE_FRAME);
    assert_eq!(flowrt_app::messages::PlanResponse::decode_frame(&frame).unwrap(), value);
}

#[test]
fn plan_response_empty_variable_fields_frame_codec() {
    let value = sample_plan_response_empty();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PLAN_RESPONSE_EMPTY_FRAME);
    assert_eq!(flowrt_app::messages::PlanResponse::decode_frame(&frame).unwrap(), value);
}

#[test]
fn plan_response_rejects_malformed_frame_decode() {
    let truncated = &EXPECTED_PLAN_RESPONSE_FRAME[..8];
    assert!(flowrt_app::messages::PlanResponse::decode_frame(truncated).unwrap_err().to_string().contains("wire payload size mismatch"));
    let offset_overflow = corrupt_var_span(EXPECTED_PLAN_RESPONSE_FRAME.to_vec(), 1, u32::MAX, 1);
    assert!(flowrt_app::messages::PlanResponse::decode_frame(&offset_overflow).unwrap_err().to_string().contains("variable tail blocks are not canonical"));
    let length_overflow = corrupt_var_span(EXPECTED_PLAN_RESPONSE_FRAME.to_vec(), 1, 0, u32::MAX);
    assert!(flowrt_app::messages::PlanResponse::decode_frame(&length_overflow).unwrap_err().to_string().contains("variable span exceeds frame tail length"));
    let mut invalid_utf8 = EXPECTED_PLAN_RESPONSE_FRAME.to_vec();
    invalid_utf8[9] = 0xff;
    assert!(flowrt_app::messages::PlanResponse::decode_frame(&invalid_utf8).unwrap_err().to_string().contains("string field is not valid UTF-8"));
}

