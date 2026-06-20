// FlowRT 管理产物。不要手工修改。

fn bytes_of<T>(value: &T) -> Vec<u8> {
    let mut bytes = vec![0u8; std::mem::size_of::<T>()];
    // Safety：生成测试只传入 FlowRT ABI v0.1 plain-data 消息，且 padding 已初始化。
    unsafe {
        std::ptr::copy_nonoverlapping(
            (value as *const T).cast::<u8>(),
            bytes.as_mut_ptr(),
            bytes.len(),
        );
    }
    bytes
}

fn assert_default_bytes_zero<T: Copy + Default>() {
    let value = T::default();
    assert_eq!(bytes_of(&value), vec![0u8; std::mem::size_of::<T>()]);
}

fn assert_byte_roundtrip<T: Copy + Default>(value: T) {
    let bytes = bytes_of(&value);
    let mut roundtrip = T::default();
    // Safety：`roundtrip` 是有效 plain-data 存储，`bytes` 长度等于 `size_of::<T>()`。
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            (&mut roundtrip as *mut T).cast::<u8>(),
            bytes.len(),
        );
    }
    assert_eq!(bytes_of(&roundtrip), bytes);
}

fn assert_sample_bytes<T: Copy>(value: T, expected: &[u8]) {
    assert_eq!(bytes_of(&value), expected);
}

const EXPECTED_PLAN_FEEDBACK_BYTES: &[u8] = &[0, 0, 16, 64];
const EXPECTED_PLAN_RESULT_BYTES: &[u8] = &[1];

fn sample_plan_feedback() -> flowrt_app::messages::PlanFeedback {
    let mut value = flowrt_app::messages::PlanFeedback::default();
    value.progress = 2.25f32;
    value
}

fn sample_plan_result() -> flowrt_app::messages::PlanResult {
    let mut value = flowrt_app::messages::PlanResult::default();
    value.accepted = true;
    value
}

#[test]
fn plan_feedback_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::PlanFeedback>(), 4);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::PlanFeedback>(), 4);
    assert_default_bytes_zero::<flowrt_app::messages::PlanFeedback>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::PlanFeedback, progress), 0);
    assert_byte_roundtrip(sample_plan_feedback());
    assert_sample_bytes(sample_plan_feedback(), EXPECTED_PLAN_FEEDBACK_BYTES);
}

#[test]
fn plan_result_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::PlanResult>(), 1);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::PlanResult>(), 1);
    assert_default_bytes_zero::<flowrt_app::messages::PlanResult>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::PlanResult, accepted), 0);
    assert_byte_roundtrip(sample_plan_result());
    assert_sample_bytes(sample_plan_result(), EXPECTED_PLAN_RESULT_BYTES);
}
