// FlowRT 管理产物。不要手工修改。

use flowrt::WireCodec;

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

const EXPECTED_CMD_BYTES: &[u8] = &[0, 0, 0, 0, 0, 0, 2, 64];
const EXPECTED_STATE_BYTES: &[u8] = &[0, 0, 0, 0, 0, 0, 2, 64];

fn sample_cmd() -> flowrt_app::messages::Cmd {
    let mut value = flowrt_app::messages::Cmd::default();
    value.u = 2.25f64;
    value
}

fn sample_state() -> flowrt_app::messages::State {
    let mut value = flowrt_app::messages::State::default();
    value.x = 2.25f64;
    value
}

#[test]
fn cmd_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::Cmd>(), 8);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::Cmd>(), 8);
    assert_default_bytes_zero::<flowrt_app::messages::Cmd>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::Cmd, u), 0);
    assert_byte_roundtrip(sample_cmd());
    assert_sample_bytes(sample_cmd(), EXPECTED_CMD_BYTES);
}

#[test]
fn cmd_wire_codec_omits_native_padding() {
    let value = sample_cmd();
    let wire = value.to_wire_vec().unwrap();
    assert_eq!(wire, vec![0, 0, 0, 0, 0, 0, 2, 64]);
    assert_eq!(flowrt_app::messages::Cmd::decode_wire(&wire).unwrap(), value);
}

#[test]
fn state_message_abi() {
    assert_eq!(std::mem::size_of::<flowrt_app::messages::State>(), 8);
    assert_eq!(std::mem::align_of::<flowrt_app::messages::State>(), 8);
    assert_default_bytes_zero::<flowrt_app::messages::State>();
    assert_eq!(std::mem::offset_of!(flowrt_app::messages::State, x), 0);
    assert_byte_roundtrip(sample_state());
    assert_sample_bytes(sample_state(), EXPECTED_STATE_BYTES);
}

#[test]
fn state_wire_codec_omits_native_padding() {
    let value = sample_state();
    let wire = value.to_wire_vec().unwrap();
    assert_eq!(wire, vec![0, 0, 0, 0, 0, 0, 2, 64]);
    assert_eq!(flowrt_app::messages::State::decode_wire(&wire).unwrap(), value);
}
