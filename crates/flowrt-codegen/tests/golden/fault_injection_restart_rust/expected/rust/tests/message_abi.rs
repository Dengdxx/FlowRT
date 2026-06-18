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

const EXPECTED_SAMPLE_BYTES: &[u8] = &[2, 0, 0, 0];

fn sample_sample() -> flowrt_app::messages::Sample {
    let mut value = flowrt_app::messages::Sample::default();
    value.value = 2u32;
    value
}

#[test]
fn sample_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::Sample>(), 4);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::Sample>(), 4);
    assert_default_bytes_zero::<flowrt_app::messages::Sample>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::Sample, value), 0);
    assert_byte_roundtrip(sample_sample());
    assert_sample_bytes(sample_sample(), EXPECTED_SAMPLE_BYTES);
}

