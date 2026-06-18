// FlowRT 管理产物。不要手工修改。

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub value: u32,
}

impl Default for Sample {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for Sample {
    const WIRE_SIZE: usize = 4;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        let mut cursor = 0usize;
        output[cursor..cursor + 4].copy_from_slice(&(self.value).to_le_bytes());
        cursor += 4;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut cursor = 0usize;
        let value = u32::from_le_bytes([input[cursor], input[cursor + 1], input[cursor + 2], input[cursor + 3]]);
        cursor += 4;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(Self {
            value,
        })
    }
}

