// FlowRT 管理产物。不要手工修改。

use flowrt::ZeroCopySend;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanRequest")]
pub struct PlanRequest {
    pub goal: u32,
}

impl Default for PlanRequest {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for PlanRequest {
    const WIRE_SIZE: usize = 4;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        let mut cursor = 0usize;
        output[cursor..cursor + 4].copy_from_slice(&(self.goal).to_le_bytes());
        cursor += 4;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut cursor = 0usize;
        let goal = u32::from_le_bytes([input[cursor], input[cursor + 1], input[cursor + 2], input[cursor + 3]]);
        cursor += 4;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(Self {
            goal,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanResponse")]
pub struct PlanResponse {
    pub accepted: bool,
}

impl Default for PlanResponse {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for PlanResponse {
    const WIRE_SIZE: usize = 1;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        let mut cursor = 0usize;
        output[cursor] = self.accepted as u8;
        cursor += 1;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut cursor = 0usize;
        let accepted = input[cursor] != 0;
        cursor += 1;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(Self {
            accepted,
        })
    }
}
