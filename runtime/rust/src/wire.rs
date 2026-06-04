/// canonical wire codec 的错误。
///
/// 该错误只描述 FlowRT wire payload 本身的问题，不暴露具体 backend 或 transport API。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireCodecError {
    /// codec 期望的 wire payload 字节数。
    pub expected: usize,
    /// 调用方提供的字节数。
    pub actual: usize,
}

impl WireCodecError {
    /// 构造 payload size mismatch 错误。
    pub const fn wrong_size(expected: usize, actual: usize) -> Self {
        Self { expected, actual }
    }
}

impl std::fmt::Display for WireCodecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "wire payload size mismatch: expected {} bytes, got {} bytes",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for WireCodecError {}

/// FlowRT canonical wire codec。
///
/// `WireCodec` 面向跨主机 copy transport：编码结果按 Contract IR 字段顺序写入 little-endian
/// primitive bytes，不包含 native struct padding。用户组件不直接调用该 trait；generated shell 和
/// backend endpoint 在内部使用它。
pub trait WireCodec: Sized {
    /// canonical wire payload 固定字节数。
    const WIRE_SIZE: usize;

    /// 把当前值编码到调用方提供的 output buffer。
    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError>;

    /// 从 canonical wire payload 解码当前类型。
    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError>;

    /// 编码到新分配的 byte vector。
    fn to_wire_vec(&self) -> Result<Vec<u8>, WireCodecError> {
        let mut output = vec![0u8; Self::WIRE_SIZE];
        self.encode_wire(&mut output)?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    struct Tiny {
        value: u16,
    }

    impl WireCodec for Tiny {
        const WIRE_SIZE: usize = 2;

        fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
            if output.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
            }
            output.copy_from_slice(&self.value.to_le_bytes());
            Ok(())
        }

        fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
            if input.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
            }
            Ok(Self {
                value: u16::from_le_bytes([input[0], input[1]]),
            })
        }
    }

    #[test]
    fn wire_codec_reports_wrong_buffer_size() {
        let error = Tiny { value: 7 }.encode_wire(&mut [0u8; 1]).unwrap_err();
        assert_eq!(error.expected, 2);
        assert_eq!(error.actual, 1);
        assert_eq!(
            error.to_string(),
            "wire payload size mismatch: expected 2 bytes, got 1 bytes"
        );
    }

    #[test]
    fn wire_codec_can_encode_to_vec_and_decode() {
        let value = Tiny { value: 0x1234 };
        let bytes = value.to_wire_vec().unwrap();
        assert_eq!(bytes, vec![0x34, 0x12]);
        assert_eq!(Tiny::decode_wire(&bytes).unwrap(), value);
    }
}
