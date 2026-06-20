// FlowRT 管理产物。不要手工修改。

use flowrt::ZeroCopySend;

#[derive(Clone, Debug, PartialEq)]
pub struct PlanRequest {
    pub goal: u32,
    pub label: String,
    pub samples: Vec<u32>,
}

impl Default for PlanRequest {
    fn default() -> Self {
        Self {
            goal: Default::default(),
            label: Default::default(),
            samples: Default::default(),
        }
    }
}

impl flowrt::FrameCodec for PlanRequest {
    fn encoded_frame_size(&self) -> usize {
        20 + self.label.len()
 + self.samples.len() * 4
    }

    fn encode_frame(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        let mut tail = Vec::<u8>::new();
        if self.label.len() > 8 {
            return Err(flowrt::WireCodecError::invalid_frame("field PlanRequest.label exceeds max 8"));
        }
        let label_span = flowrt::append_tail_block(&mut tail, self.label.as_bytes())?;
        if self.samples.len() > 4 {
            return Err(flowrt::WireCodecError::invalid_frame("field PlanRequest.samples exceeds max 4"));
        }
        let mut samples_tail = Vec::<u8>::with_capacity(self.samples.len() * 4);
        for element in &self.samples {
            let start = samples_tail.len();
            samples_tail.resize(start + 4, 0);
            let mut cursor = start;
            samples_tail[cursor..cursor + 4].copy_from_slice(&(*element).to_le_bytes());
            cursor += 4;
            let _ = cursor;
        }
        let samples_span = flowrt::append_tail_block(&mut tail, &samples_tail)?;
        if output.len() != self.encoded_frame_size() {
            return Err(flowrt::WireCodecError::wrong_size(self.encoded_frame_size(), output.len()));
        }
        let mut cursor = 0usize;
        output[cursor..cursor + 4].copy_from_slice(&(self.goal).to_le_bytes());
        cursor += 4;
        label_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        samples_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        output[20..].copy_from_slice(&tail);
        let _ = cursor;
        Ok(())
    }

    fn decode_frame(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() < 20 {
            return Err(flowrt::WireCodecError::wrong_size(20, input.len()));
        }
        let mut cursor = 0usize;
        let goal = u32::from_le_bytes([input[cursor], input[cursor + 1], input[cursor + 2], input[cursor + 3]]);
        cursor += 4;
        let label_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let samples_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let _ = cursor;
        let mut decoder = flowrt::FrameDecoder::new(&input[20..]);
        let label = String::from_utf8(decoder.read_block(label_span)?.to_vec()).map_err(|_| flowrt::WireCodecError::invalid_frame("string field is not valid UTF-8"))?;
        let samples_block = decoder.read_block(samples_span)?;
        if samples_block.len() % 4 != 0 {
            return Err(flowrt::WireCodecError::invalid_frame("sequence byte length is not divisible by element wire size"));
        }
        let mut samples = Vec::<u32>::with_capacity(samples_block.len() / 4);
        for chunk in samples_block.chunks_exact(4) {
            samples.push(<u32 as flowrt::WireCodec>::decode_wire(chunk)?);
        }
        decoder.finish()?;
        Ok(Self {
            goal,
            label,
            samples,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanResponse {
    pub accepted: bool,
    pub detail: String,
}

impl Default for PlanResponse {
    fn default() -> Self {
        Self {
            accepted: Default::default(),
            detail: Default::default(),
        }
    }
}

impl flowrt::FrameCodec for PlanResponse {
    fn encoded_frame_size(&self) -> usize {
        9 + self.detail.len()
    }

    fn encode_frame(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        let mut tail = Vec::<u8>::new();
        if self.detail.len() > 12 {
            return Err(flowrt::WireCodecError::invalid_frame("field PlanResponse.detail exceeds max 12"));
        }
        let detail_span = flowrt::append_tail_block(&mut tail, self.detail.as_bytes())?;
        if output.len() != self.encoded_frame_size() {
            return Err(flowrt::WireCodecError::wrong_size(self.encoded_frame_size(), output.len()));
        }
        let mut cursor = 0usize;
        output[cursor] = self.accepted as u8;
        cursor += 1;
        detail_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        output[9..].copy_from_slice(&tail);
        let _ = cursor;
        Ok(())
    }

    fn decode_frame(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() < 9 {
            return Err(flowrt::WireCodecError::wrong_size(9, input.len()));
        }
        let mut cursor = 0usize;
        let accepted = input[cursor] != 0;
        cursor += 1;
        let detail_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let _ = cursor;
        let mut decoder = flowrt::FrameDecoder::new(&input[9..]);
        let detail = String::from_utf8(decoder.read_block(detail_span)?.to_vec()).map_err(|_| flowrt::WireCodecError::invalid_frame("string field is not valid UTF-8"))?;
        decoder.finish()?;
        Ok(Self {
            accepted,
            detail,
        })
    }
}
