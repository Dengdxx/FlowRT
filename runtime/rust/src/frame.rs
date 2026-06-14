//! FlowRT canonical frame codec 与变长消息运行时工具。
//!
//! `WireCodec` 继续表示固定长度 canonical payload。`FrameCodec` 表示可变长度 canonical
//! frame：固定 header 在前，变长 tail 紧随其后，变长字段用 offset/len 描述 tail 中的块。

use crate::{WireCodec, WireCodecError};

/// canonical frame 中一个变长字段的描述符大小。
pub const VAR_SPAN_WIRE_SIZE: usize = 8;

/// 变长字段在 tail 中的位置。
///
/// `offset` 以 tail 起点为原点，`len` 使用字节单位。空值 canonical 表示为 `{0, 0}`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VarSpan {
    offset: u32,
    len: u32,
}

impl VarSpan {
    /// 构造一个 span。
    pub const fn new(offset: u32, len: u32) -> Self {
        Self { offset, len }
    }

    /// 返回 tail-relative offset。
    pub const fn offset(self) -> u32 {
        self.offset
    }

    /// 返回 byte length。
    pub const fn len(self) -> u32 {
        self.len
    }

    /// 判断 span 是否为空。
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// 写入 little-endian descriptor。
    pub fn encode(self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != VAR_SPAN_WIRE_SIZE {
            return Err(WireCodecError::wrong_size(VAR_SPAN_WIRE_SIZE, output.len()));
        }
        output[..4].copy_from_slice(&self.offset.to_le_bytes());
        output[4..].copy_from_slice(&self.len.to_le_bytes());
        Ok(())
    }

    /// 从 little-endian descriptor 读取 span。
    pub fn decode(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != VAR_SPAN_WIRE_SIZE {
            return Err(WireCodecError::wrong_size(VAR_SPAN_WIRE_SIZE, input.len()));
        }
        Ok(Self {
            offset: u32::from_le_bytes([input[0], input[1], input[2], input[3]]),
            len: u32::from_le_bytes([input[4], input[5], input[6], input[7]]),
        })
    }
}

/// FlowRT canonical frame codec。
///
/// 生成的 backend shell 使用该 trait 传输跨主机 payload。固定消息通过 `WireCodec` 自动实现；
/// 变长消息由 codegen 生成动态 frame 实现。
pub trait FrameCodec: Sized {
    /// 该值编码后的实际 frame 字节数。
    fn encoded_frame_size(&self) -> usize;

    /// 将当前值编码到调用方提供的 frame buffer。
    fn encode_frame(&self, output: &mut [u8]) -> Result<(), WireCodecError>;

    /// 从 canonical frame 解码当前类型。
    fn decode_frame(input: &[u8]) -> Result<Self, WireCodecError>;

    /// 编码到新分配的 byte vector。
    fn to_frame_vec(&self) -> Result<Vec<u8>, WireCodecError> {
        let mut output = vec![0u8; self.encoded_frame_size()];
        self.encode_frame(&mut output)?;
        Ok(output)
    }
}

impl<T> FrameCodec for T
where
    T: WireCodec,
{
    fn encoded_frame_size(&self) -> usize {
        T::WIRE_SIZE
    }

    fn encode_frame(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        self.encode_wire(output)
    }

    fn decode_frame(input: &[u8]) -> Result<Self, WireCodecError> {
        T::decode_wire(input)
    }
}

/// 帮助 codegen 顺序验证 variable tail 的 decoder。
pub struct FrameDecoder<'a> {
    tail: &'a [u8],
    cursor: usize,
}

impl<'a> FrameDecoder<'a> {
    /// 构造一个 tail decoder。
    pub fn new(tail: &'a [u8]) -> Self {
        Self { tail, cursor: 0 }
    }

    /// 按 canonical 顺序读取一个变长块。
    pub fn read_block(&mut self, span: VarSpan) -> Result<&'a [u8], WireCodecError> {
        let len = span.len() as usize;
        if len == 0 {
            if span.offset() != 0 {
                return Err(WireCodecError::invalid_frame(
                    "empty variable span must use zero offset",
                ));
            }
            return Ok(&[]);
        }
        let offset = span.offset() as usize;
        if offset != self.cursor {
            return Err(WireCodecError::invalid_frame(
                "variable tail blocks are not canonical",
            ));
        }
        let end = offset
            .checked_add(len)
            .ok_or_else(|| WireCodecError::invalid_frame("variable span overflows usize"))?;
        if end > self.tail.len() {
            return Err(WireCodecError::invalid_frame(
                "variable span exceeds frame tail length",
            ));
        }
        self.cursor = end;
        Ok(&self.tail[offset..end])
    }

    /// 完成解码并拒绝 tail trailing bytes。
    pub fn finish(self) -> Result<(), WireCodecError> {
        if self.cursor != self.tail.len() {
            return Err(WireCodecError::invalid_frame(
                "variable frame contains trailing tail bytes",
            ));
        }
        Ok(())
    }
}

/// 将一个变长块追加到 tail，并返回对应 descriptor。
pub fn append_tail_block(tail: &mut Vec<u8>, bytes: &[u8]) -> Result<VarSpan, WireCodecError> {
    if bytes.is_empty() {
        return Ok(VarSpan::default());
    }
    let offset = u32::try_from(tail.len())
        .map_err(|_| WireCodecError::invalid_frame("variable tail offset exceeds u32"))?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| WireCodecError::invalid_frame("variable block length exceeds u32"))?;
    tail.extend_from_slice(bytes);
    Ok(VarSpan::new(offset, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    struct Tiny(u16);

    impl WireCodec for Tiny {
        const WIRE_SIZE: usize = 2;

        fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
            if output.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
            }
            output.copy_from_slice(&self.0.to_le_bytes());
            Ok(())
        }

        fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
            if input.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
            }
            Ok(Self(u16::from_le_bytes([input[0], input[1]])))
        }
    }

    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    struct Point {
        x: f32,
        y: f32,
    }

    impl WireCodec for Point {
        const WIRE_SIZE: usize = 8;

        fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
            if output.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
            }
            output[0..4].copy_from_slice(&self.x.to_le_bytes());
            output[4..8].copy_from_slice(&self.y.to_le_bytes());
            Ok(())
        }

        fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
            if input.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
            }
            Ok(Self {
                x: f32::from_le_bytes([input[0], input[1], input[2], input[3]]),
                y: f32::from_le_bytes([input[4], input[5], input[6], input[7]]),
            })
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct VariableFrame {
        label: String,
        payload: Vec<u8>,
        points: Vec<Point>,
    }

    impl FrameCodec for VariableFrame {
        fn encoded_frame_size(&self) -> usize {
            VAR_SPAN_WIRE_SIZE * 3
                + self.label.len()
                + self.payload.len()
                + self.points.len() * Point::WIRE_SIZE
        }

        fn encode_frame(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
            let mut tail = Vec::new();
            let label_span = append_tail_block(&mut tail, self.label.as_bytes())?;
            let payload_span = append_tail_block(&mut tail, &self.payload)?;
            let mut points_tail = Vec::with_capacity(self.points.len() * Point::WIRE_SIZE);
            for point in &self.points {
                let start = points_tail.len();
                points_tail.resize(start + Point::WIRE_SIZE, 0);
                point.encode_wire(&mut points_tail[start..start + Point::WIRE_SIZE])?;
            }
            let points_span = append_tail_block(&mut tail, &points_tail)?;
            if output.len() != self.encoded_frame_size() {
                return Err(WireCodecError::wrong_size(
                    self.encoded_frame_size(),
                    output.len(),
                ));
            }
            label_span.encode(&mut output[0..8])?;
            payload_span.encode(&mut output[8..16])?;
            points_span.encode(&mut output[16..24])?;
            output[24..].copy_from_slice(&tail);
            Ok(())
        }

        fn decode_frame(input: &[u8]) -> Result<Self, WireCodecError> {
            if input.len() < 24 {
                return Err(WireCodecError::wrong_size(24, input.len()));
            }
            let label_span = VarSpan::decode(&input[0..8])?;
            let payload_span = VarSpan::decode(&input[8..16])?;
            let points_span = VarSpan::decode(&input[16..24])?;
            let mut decoder = FrameDecoder::new(&input[24..]);
            let label = String::from_utf8(decoder.read_block(label_span)?.to_vec())
                .map_err(|_| WireCodecError::invalid_frame("string field is not valid UTF-8"))?;
            let payload = decoder.read_block(payload_span)?.to_vec();
            let points_block = decoder.read_block(points_span)?;
            if points_block.len() % Point::WIRE_SIZE != 0 {
                return Err(WireCodecError::invalid_frame(
                    "sequence byte length is not divisible by element wire size",
                ));
            }
            let mut points = Vec::with_capacity(points_block.len() / Point::WIRE_SIZE);
            for chunk in points_block.chunks_exact(Point::WIRE_SIZE) {
                points.push(Point::decode_wire(chunk)?);
            }
            decoder.finish()?;
            Ok(Self {
                label,
                payload,
                points,
            })
        }
    }

    const EXPECTED_VARIABLE_FRAME_BYTES: &[u8] = &[
        0, 0, 0, 0, 9, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0, 12, 0, 0, 0, 16, 0, 0, 0, 117, 116, 102,
        56, 45, 206, 188, 45, 50, 3, 4, 5, 0, 0, 128, 64, 0, 0, 144, 64, 0, 0, 160, 64, 0, 0, 176,
        64,
    ];

    fn sample_variable_frame() -> VariableFrame {
        VariableFrame {
            label: "utf8-\u{03bc}-2".to_string(),
            payload: vec![3, 4, 5],
            points: vec![Point { x: 4.0, y: 4.5 }, Point { x: 5.0, y: 5.5 }],
        }
    }

    fn corrupt_span(mut frame: Vec<u8>, offset: usize, span_offset: u32, len: u32) -> Vec<u8> {
        frame[offset..offset + 4].copy_from_slice(&span_offset.to_le_bytes());
        frame[offset + 4..offset + 8].copy_from_slice(&len.to_le_bytes());
        frame
    }

    #[test]
    fn fixed_wire_codec_adapts_to_frame_codec() {
        let value = Tiny(0x1234);
        assert_eq!(value.encoded_frame_size(), 2);
        assert_eq!(value.to_frame_vec().unwrap(), [0x34, 0x12]);
        assert_eq!(Tiny::decode_frame(&[0x34, 0x12]).unwrap(), value);
    }

    #[test]
    fn frame_decoder_rejects_noncanonical_tail_layout() {
        let mut tail = Vec::new();
        let first = append_tail_block(&mut tail, &[1, 2]).unwrap();
        let second = append_tail_block(&mut tail, &[3]).unwrap();
        let mut decoder = FrameDecoder::new(&tail);
        assert_eq!(decoder.read_block(first).unwrap(), [1, 2]);
        assert_eq!(decoder.read_block(second).unwrap(), [3]);
        decoder.finish().unwrap();

        let mut bad = FrameDecoder::new(&tail);
        assert!(bad.read_block(VarSpan::new(1, 1)).is_err());
    }

    #[test]
    fn variable_frame_span_roundtrip_uses_canonical_tail_order() {
        let mut tail = Vec::new();
        let payload = append_tail_block(&mut tail, &[0xAA, 0xBB]).unwrap();
        let label = append_tail_block(&mut tail, b"ok").unwrap();
        let empty = append_tail_block(&mut tail, &[]).unwrap();

        let mut header = [0u8; VAR_SPAN_WIRE_SIZE * 3];
        payload.encode(&mut header[0..VAR_SPAN_WIRE_SIZE]).unwrap();
        label
            .encode(&mut header[VAR_SPAN_WIRE_SIZE..VAR_SPAN_WIRE_SIZE * 2])
            .unwrap();
        empty.encode(&mut header[VAR_SPAN_WIRE_SIZE * 2..]).unwrap();

        assert_eq!(
            header,
            [
                0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        assert_eq!(tail, [0xAA, 0xBB, b'o', b'k']);

        let payload_span = VarSpan::decode(&header[0..VAR_SPAN_WIRE_SIZE]).unwrap();
        let label_span =
            VarSpan::decode(&header[VAR_SPAN_WIRE_SIZE..VAR_SPAN_WIRE_SIZE * 2]).unwrap();
        let empty_span = VarSpan::decode(&header[VAR_SPAN_WIRE_SIZE * 2..]).unwrap();
        let mut decoder = FrameDecoder::new(&tail);
        assert_eq!(decoder.read_block(payload_span).unwrap(), [0xAA, 0xBB]);
        assert_eq!(decoder.read_block(label_span).unwrap(), b"ok");
        assert!(decoder.read_block(empty_span).unwrap().is_empty());
        decoder.finish().unwrap();
    }

    #[test]
    fn variable_frame_codec_roundtrips_utf8_bytes_and_struct_sequence() {
        let value = sample_variable_frame();
        let encoded = value.to_frame_vec().unwrap();
        assert_eq!(encoded, EXPECTED_VARIABLE_FRAME_BYTES);
        assert_eq!(VariableFrame::decode_frame(&encoded).unwrap(), value);
    }

    #[test]
    fn variable_frame_codec_roundtrips_empty_string_bytes_and_sequence() {
        let value = VariableFrame {
            label: String::new(),
            payload: Vec::new(),
            points: Vec::new(),
        };
        let encoded = value.to_frame_vec().unwrap();
        assert_eq!(encoded, [0; VAR_SPAN_WIRE_SIZE * 3]);
        assert_eq!(VariableFrame::decode_frame(&encoded).unwrap(), value);
    }

    #[test]
    fn variable_frame_decode_reports_truncation_offset_and_length_errors() {
        let expected = EXPECTED_VARIABLE_FRAME_BYTES;
        let truncated = VariableFrame::decode_frame(&expected[..23]).unwrap_err();
        assert_eq!(
            truncated.to_string(),
            "wire payload size mismatch: expected 24 bytes, got 23 bytes"
        );

        let offset_overflow = corrupt_span(expected.to_vec(), 0, u32::MAX, 1);
        assert_eq!(
            VariableFrame::decode_frame(&offset_overflow)
                .unwrap_err()
                .to_string(),
            "variable tail blocks are not canonical"
        );

        let length_overflow = corrupt_span(expected.to_vec(), 0, 0, u32::MAX);
        assert_eq!(
            VariableFrame::decode_frame(&length_overflow)
                .unwrap_err()
                .to_string(),
            "variable span exceeds frame tail length"
        );
    }

    #[test]
    fn variable_frame_decode_rejects_invalid_utf8_string() {
        let mut frame = EXPECTED_VARIABLE_FRAME_BYTES.to_vec();
        frame[24] = 0xff;
        assert_eq!(
            VariableFrame::decode_frame(&frame).unwrap_err().to_string(),
            "string field is not valid UTF-8"
        );
    }
}
