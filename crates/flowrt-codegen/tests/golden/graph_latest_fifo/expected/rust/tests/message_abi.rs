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

const EXPECTED_IMU_BYTES: &[u8] = &[0, 0, 16, 64];
const EXPECTED_ODOM_BYTES: &[u8] = &[0, 0, 16, 64];

fn sample_imu() -> flowrt_app::messages::Imu {
    let mut value = flowrt_app::messages::Imu::default();
    value.ax = 2.25f32;
    value
}

fn sample_odom() -> flowrt_app::messages::Odom {
    let mut value = flowrt_app::messages::Odom::default();
    value.x = 2.25f32;
    value
}

#[test]
fn imu_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::Imu>(), 4);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::Imu>(), 4);
    assert_default_bytes_zero::<flowrt_app::messages::Imu>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::Imu, ax), 0);
    assert_byte_roundtrip(sample_imu());
    assert_sample_bytes(sample_imu(), EXPECTED_IMU_BYTES);
}

#[test]
fn odom_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::Odom>(), 4);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::Odom>(), 4);
    assert_default_bytes_zero::<flowrt_app::messages::Odom>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::Odom, x), 0);
    assert_byte_roundtrip(sample_odom());
    assert_sample_bytes(sample_odom(), EXPECTED_ODOM_BYTES);
}
