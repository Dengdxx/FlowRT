// FlowRT 管理产物。不要手工修改。

use flowrt::FrameCodec;

fn corrupt_var_span(mut frame: Vec<u8>, header_offset: usize, offset: u32, len: u32) -> Vec<u8> {
    frame[header_offset..header_offset + 4].copy_from_slice(&offset.to_le_bytes());
    frame[header_offset + 4..header_offset + 8].copy_from_slice(&len.to_le_bytes());
    frame
}

const EXPECTED_PACKET_FRAME: &[u8] = &[0, 0, 0, 0, 3, 0, 0, 0, 3, 0, 0, 0, 9, 0, 0, 0, 12, 0, 0, 0, 8, 0, 0, 0, 2, 3, 4, 117, 116, 102, 56, 45, 206, 188, 45, 51, 5, 0, 0, 0, 6, 0, 0, 0];
const EXPECTED_PACKET_EMPTY_FRAME: &[u8] = &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn sample_packet() -> flowrt_app::messages::Packet {
    flowrt_app::messages::Packet {
        payload: vec![2u8, 3u8, 4u8],
        label: "utf8-\u{03bc}-3".to_string(),
        samples: vec![5u32, 6u32],
    }
}

fn sample_packet_empty() -> flowrt_app::messages::Packet {
    flowrt_app::messages::Packet {
        payload: Vec::new(),
        label: String::new(),
        samples: Vec::new(),
    }
}

#[test]
fn packet_canonical_frame_codec() {
    let value = sample_packet();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PACKET_FRAME);
    assert_eq!(flowrt_app::messages::Packet::decode_frame(&frame).unwrap(), value);
}

#[test]
fn packet_empty_variable_fields_frame_codec() {
    let value = sample_packet_empty();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PACKET_EMPTY_FRAME);
    assert_eq!(flowrt_app::messages::Packet::decode_frame(&frame).unwrap(), value);
}

#[test]
fn packet_rejects_malformed_frame_decode() {
    let truncated = &EXPECTED_PACKET_FRAME[..23];
    assert!(flowrt_app::messages::Packet::decode_frame(truncated).unwrap_err().to_string().contains("wire payload size mismatch"));
    let offset_overflow = corrupt_var_span(EXPECTED_PACKET_FRAME.to_vec(), 0, u32::MAX, 1);
    assert!(flowrt_app::messages::Packet::decode_frame(&offset_overflow).unwrap_err().to_string().contains("variable tail blocks are not canonical"));
    let length_overflow = corrupt_var_span(EXPECTED_PACKET_FRAME.to_vec(), 0, 0, u32::MAX);
    assert!(flowrt_app::messages::Packet::decode_frame(&length_overflow).unwrap_err().to_string().contains("variable span exceeds frame tail length"));
    let mut invalid_utf8 = EXPECTED_PACKET_FRAME.to_vec();
    invalid_utf8[27] = 0xff;
    assert!(flowrt_app::messages::Packet::decode_frame(&invalid_utf8).unwrap_err().to_string().contains("string field is not valid UTF-8"));
}

