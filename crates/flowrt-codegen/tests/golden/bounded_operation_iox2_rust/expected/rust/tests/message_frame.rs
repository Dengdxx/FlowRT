// FlowRT 管理产物。不要手工修改。

use flowrt::FrameCodec;

fn corrupt_var_span(mut frame: Vec<u8>, header_offset: usize, offset: u32, len: u32) -> Vec<u8> {
    frame[header_offset..header_offset + 4].copy_from_slice(&offset.to_le_bytes());
    frame[header_offset + 4..header_offset + 8].copy_from_slice(&len.to_le_bytes());
    frame
}

const EXPECTED_PLAN_GOAL_FRAME: &[u8] = &[0, 0, 0, 0, 9, 0, 0, 0, 117, 116, 102, 56, 45, 206, 188, 45, 50];
const EXPECTED_PLAN_GOAL_EMPTY_FRAME: &[u8] = &[0, 0, 0, 0, 0, 0, 0, 0];

fn sample_plan_goal() -> flowrt_app::messages::PlanGoal {
    flowrt_app::messages::PlanGoal {
        target: "utf8-\u{03bc}-2".to_string(),
    }
}

fn sample_plan_goal_empty() -> flowrt_app::messages::PlanGoal {
    flowrt_app::messages::PlanGoal {
        target: String::new(),
    }
}

#[test]
fn plan_goal_canonical_frame_codec() {
    let value = sample_plan_goal();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PLAN_GOAL_FRAME);
    assert_eq!(flowrt_app::messages::PlanGoal::decode_frame(&frame).unwrap(), value);
}

#[test]
fn plan_goal_empty_variable_fields_frame_codec() {
    let value = sample_plan_goal_empty();
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(frame, EXPECTED_PLAN_GOAL_EMPTY_FRAME);
    assert_eq!(flowrt_app::messages::PlanGoal::decode_frame(&frame).unwrap(), value);
}

#[test]
fn plan_goal_rejects_malformed_frame_decode() {
    let truncated = &EXPECTED_PLAN_GOAL_FRAME[..7];
    assert!(flowrt_app::messages::PlanGoal::decode_frame(truncated).unwrap_err().to_string().contains("wire payload size mismatch"));
    let offset_overflow = corrupt_var_span(EXPECTED_PLAN_GOAL_FRAME.to_vec(), 0, u32::MAX, 1);
    assert!(flowrt_app::messages::PlanGoal::decode_frame(&offset_overflow).unwrap_err().to_string().contains("variable tail blocks are not canonical"));
    let length_overflow = corrupt_var_span(EXPECTED_PLAN_GOAL_FRAME.to_vec(), 0, 0, u32::MAX);
    assert!(flowrt_app::messages::PlanGoal::decode_frame(&length_overflow).unwrap_err().to_string().contains("variable span exceeds frame tail length"));
    let mut invalid_utf8 = EXPECTED_PLAN_GOAL_FRAME.to_vec();
    invalid_utf8[8] = 0xff;
    assert!(flowrt_app::messages::PlanGoal::decode_frame(&invalid_utf8).unwrap_err().to_string().contains("string field is not valid UTF-8"));
}

