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

macro_rules! impl_primitive_wire_codec {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl WireCodec for $ty {
                const WIRE_SIZE: usize = std::mem::size_of::<Self>();

                fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
                    if output.len() != Self::WIRE_SIZE {
                        return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
                    }
                    output.copy_from_slice(&self.to_le_bytes());
                    Ok(())
                }

                fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
                    if input.len() != Self::WIRE_SIZE {
                        return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
                    }
                    Ok(Self::from_le_bytes(
                        input
                            .try_into()
                            .expect("wire size was checked before primitive decode"),
                    ))
                }
            }
        )+
    };
}

impl_primitive_wire_codec!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

impl WireCodec for bool {
    const WIRE_SIZE: usize = 1;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        output[0] = u8::from(*self);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(input[0] != 0)
    }
}

impl<T, const N: usize> WireCodec for [T; N]
where
    T: WireCodec + Copy + Default,
{
    const WIRE_SIZE: usize = T::WIRE_SIZE * N;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        for (index, value) in self.iter().enumerate() {
            let start = index * T::WIRE_SIZE;
            value.encode_wire(&mut output[start..start + T::WIRE_SIZE])?;
        }
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut values = [T::default(); N];
        for (index, value) in values.iter_mut().enumerate() {
            let start = index * T::WIRE_SIZE;
            *value = T::decode_wire(&input[start..start + T::WIRE_SIZE])?;
        }
        Ok(values)
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

    #[test]
    fn primitive_wire_codec_uses_little_endian_bytes() {
        assert_eq!(
            0x1234_5678u32.to_wire_vec().unwrap(),
            [0x78, 0x56, 0x34, 0x12]
        );
        assert_eq!(
            u32::decode_wire(&[0x78, 0x56, 0x34, 0x12]).unwrap(),
            0x1234_5678
        );
        assert_eq!(true.to_wire_vec().unwrap(), [1]);
        assert!(bool::decode_wire(&[1]).unwrap());
    }

    #[test]
    fn fixed_array_wire_codec_concatenates_element_payloads() {
        let value = [0x1234u16, 0x5678u16];
        assert_eq!(value.to_wire_vec().unwrap(), [0x34, 0x12, 0x78, 0x56]);
        assert_eq!(
            <[u16; 2]>::decode_wire(&[0x34, 0x12, 0x78, 0x56]).unwrap(),
            value
        );
    }
}
