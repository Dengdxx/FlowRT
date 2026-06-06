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
}
