// FlowRT 管理产物。不要手工修改。

use flowrt::ZeroCopySend;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanFeedback")]
pub struct PlanFeedback {
    pub progress: f32,
}

impl Default for PlanFeedback {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for PlanFeedback {
    const WIRE_SIZE: usize = 4;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        let mut cursor = 0usize;
        output[cursor..cursor + 4].copy_from_slice(&(self.progress).to_le_bytes());
        cursor += 4;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        let mut cursor = 0usize;
        let progress = f32::from_le_bytes([input[cursor], input[cursor + 1], input[cursor + 2], input[cursor + 3]]);
        cursor += 4;
        debug_assert_eq!(cursor, Self::WIRE_SIZE);
        Ok(Self {
            progress,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanGoal {
    pub target: String,
}

impl Default for PlanGoal {
    fn default() -> Self {
        Self {
            target: Default::default(),
        }
    }
}

impl flowrt::FrameCodec for PlanGoal {
    fn encoded_frame_size(&self) -> usize {
        8 + self.target.len()
    }

    fn encode_frame(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        let mut tail = Vec::<u8>::new();
        let target_span = flowrt::append_tail_block(&mut tail, self.target.as_bytes())?;
        if output.len() != self.encoded_frame_size() {
            return Err(flowrt::WireCodecError::wrong_size(self.encoded_frame_size(), output.len()));
        }
        let mut cursor = 0usize;
        target_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        output[8..].copy_from_slice(&tail);
        let _ = cursor;
        Ok(())
    }

    fn decode_frame(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() < 8 {
            return Err(flowrt::WireCodecError::wrong_size(8, input.len()));
        }
        let mut cursor = 0usize;
        let target_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let _ = cursor;
        let mut decoder = flowrt::FrameDecoder::new(&input[8..]);
        let target = String::from_utf8(decoder.read_block(target_span)?.to_vec()).map_err(|_| flowrt::WireCodecError::invalid_frame("string field is not valid UTF-8"))?;
        decoder.finish()?;
        Ok(Self {
            target,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanResult")]
pub struct PlanResult {
    pub accepted: bool,
}

impl Default for PlanResult {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

impl flowrt::WireCodec for PlanResult {
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
