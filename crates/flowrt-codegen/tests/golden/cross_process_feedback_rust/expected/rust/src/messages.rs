// FlowRT 管理产物。不要手工修改。

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cmd {
    pub u: f64,
}

impl Default for Cmd {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for Cmd {
    const WIRE_SIZE: usize = 8;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        let mut cursor = 0usize;
        output[cursor..cursor + 8].copy_from_slice(&(self.u).to_le_bytes());
        cursor += 8;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut cursor = 0usize;
        let u = f64::from_le_bytes([input[cursor], input[cursor + 1], input[cursor + 2], input[cursor + 3], input[cursor + 4], input[cursor + 5], input[cursor + 6], input[cursor + 7]]);
        cursor += 8;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(Self {
            u,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub x: f64,
}

impl Default for State {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for State {
    const WIRE_SIZE: usize = 8;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        let mut cursor = 0usize;
        output[cursor..cursor + 8].copy_from_slice(&(self.x).to_le_bytes());
        cursor += 8;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut cursor = 0usize;
        let x = f64::from_le_bytes([input[cursor], input[cursor + 1], input[cursor + 2], input[cursor + 3], input[cursor + 4], input[cursor + 5], input[cursor + 6], input[cursor + 7]]);
        cursor += 8;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(Self {
            x,
        })
    }
}
