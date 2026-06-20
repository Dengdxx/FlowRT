// FlowRT 管理产物。不要手工修改。

use flowrt::ZeroCopySend;

#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub payload: Vec<u8>,
    pub label: String,
    pub samples: Vec<u32>,
}

impl Default for Packet {
    fn default() -> Self {
        Self {
            payload: Default::default(),
            label: Default::default(),
            samples: Default::default(),
        }
    }
}

impl flowrt::FrameCodec for Packet {
    fn encoded_frame_size(&self) -> usize {
        24 + self.payload.len()
 + self.label.len()
 + self.samples.len() * 4
    }

    fn encode_frame(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {
        let mut tail = Vec::<u8>::new();
        let payload_span = flowrt::append_tail_block(&mut tail, self.payload.as_slice())?;
        let label_span = flowrt::append_tail_block(&mut tail, self.label.as_bytes())?;
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
        payload_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        label_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        samples_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        output[24..].copy_from_slice(&tail);
        let _ = cursor;
        Ok(())
    }

    fn decode_frame(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {
        if input.len() < 24 {
            return Err(flowrt::WireCodecError::wrong_size(24, input.len()));
        }
        let mut cursor = 0usize;
        let payload_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let label_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let samples_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        let _ = cursor;
        let mut decoder = flowrt::FrameDecoder::new(&input[24..]);
        let payload = decoder.read_block(payload_span)?.to_vec();
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
            payload,
            label,
            samples,
        })
    }
}
