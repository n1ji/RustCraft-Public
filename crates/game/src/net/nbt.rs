//! Minimal uncompressed NBT reader used for item tooltips.

use std::collections::HashMap;
use std::io;

const MAX_NBT_BYTES: usize = super::protocol::MAX_PACKET_BYTES;
const MAX_NBT_DEPTH: usize = 64;
const MAX_NBT_NODES: usize = 65_536;
const MAX_NBT_COLLECTION_LEN: usize = 65_536;

#[derive(Clone, Debug, PartialEq)]
pub enum NbtTag {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<NbtTag>),
    Compound(HashMap<String, NbtTag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl NbtTag {
    pub fn as_compound(&self) -> Option<&HashMap<String, NbtTag>> {
        match self {
            NbtTag::Compound(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[NbtTag]> {
        match self {
            NbtTag::List(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            NbtTag::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i16(&self) -> Option<i16> {
        match self {
            NbtTag::Byte(value) => Some(*value as i16),
            NbtTag::Short(value) => Some(*value),
            NbtTag::Int(value) => Some(*value as i16),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            NbtTag::Byte(value) => Some(*value as i32),
            NbtTag::Short(value) => Some(*value as i32),
            NbtTag::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            NbtTag::Byte(value) => Some(*value as i64),
            NbtTag::Short(value) => Some(*value as i64),
            NbtTag::Int(value) => Some(*value as i64),
            NbtTag::Long(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            NbtTag::Byte(value) => Some(*value as f64),
            NbtTag::Short(value) => Some(*value as f64),
            NbtTag::Int(value) => Some(*value as f64),
            NbtTag::Long(value) => Some(*value as f64),
            NbtTag::Float(value) => Some(*value as f64),
            NbtTag::Double(value) => Some(*value),
            _ => None,
        }
    }
}

pub fn parse_root(bytes: &[u8]) -> io::Result<NbtTag> {
    if bytes.len() > MAX_NBT_BYTES {
        return Err(invalid_nbt("nbt payload exceeds maximum length"));
    }
    let mut reader = NbtReader {
        bytes,
        pos: 0,
        nodes: 0,
    };
    let tag_id = reader.read_u8()?;
    if tag_id == 0 {
        return Ok(NbtTag::End);
    }
    reader.read_string()?;
    reader.read_payload(tag_id, 0)
}

struct NbtReader<'a> {
    bytes: &'a [u8],
    pos: usize,
    nodes: usize,
}

impl NbtReader<'_> {
    fn read_payload(&mut self, tag_id: u8, depth: usize) -> io::Result<NbtTag> {
        if depth > MAX_NBT_DEPTH {
            return Err(invalid_nbt("nbt nesting exceeds maximum depth"));
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| invalid_nbt("nbt node count overflow"))?;
        if self.nodes > MAX_NBT_NODES {
            return Err(invalid_nbt("nbt payload contains too many nodes"));
        }
        match tag_id {
            0 => Ok(NbtTag::End),
            1 => Ok(NbtTag::Byte(self.read_i8()?)),
            2 => Ok(NbtTag::Short(self.read_i16()?)),
            3 => Ok(NbtTag::Int(self.read_i32()?)),
            4 => Ok(NbtTag::Long(self.read_i64()?)),
            5 => Ok(NbtTag::Float(f32::from_bits(self.read_u32()?))),
            6 => Ok(NbtTag::Double(f64::from_bits(self.read_u64()?))),
            7 => {
                let len = self.read_collection_len(MAX_NBT_BYTES)?;
                let bytes = self.read_exact(len)?;
                Ok(NbtTag::ByteArray(bytes.iter().map(|b| *b as i8).collect()))
            }
            8 => Ok(NbtTag::String(self.read_string()?)),
            9 => {
                let child_id = self.read_u8()?;
                let len = self.read_collection_len(MAX_NBT_COLLECTION_LEN)?;
                if child_id == 0 && len != 0 {
                    return Err(invalid_nbt("non-empty nbt list has end tag type"));
                }
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(self.read_payload(child_id, depth + 1)?);
                }
                Ok(NbtTag::List(values))
            }
            10 => {
                let mut map = HashMap::new();
                loop {
                    let child_id = self.read_u8()?;
                    if child_id == 0 {
                        break;
                    }
                    let name = self.read_string()?;
                    let value = self.read_payload(child_id, depth + 1)?;
                    map.insert(name, value);
                }
                Ok(NbtTag::Compound(map))
            }
            11 => {
                let len = self.read_collection_len(MAX_NBT_COLLECTION_LEN)?;
                let byte_len = len
                    .checked_mul(4)
                    .ok_or_else(|| invalid_nbt("nbt int array length overflow"))?;
                if byte_len > self.remaining() {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "nbt eof"));
                }
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(self.read_i32()?);
                }
                Ok(NbtTag::IntArray(values))
            }
            12 => {
                let len = self.read_collection_len(MAX_NBT_COLLECTION_LEN)?;
                let byte_len = len
                    .checked_mul(8)
                    .ok_or_else(|| invalid_nbt("nbt long array length overflow"))?;
                if byte_len > self.remaining() {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "nbt eof"));
                }
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(self.read_i64()?);
                }
                Ok(NbtTag::LongArray(values))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown nbt tag id {}", tag_id),
            )),
        }
    }

    fn read_string(&mut self) -> io::Result<String> {
        let len = self.read_u16()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid nbt utf8"))
    }

    fn read_exact(&mut self, len: usize) -> io::Result<&[u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| invalid_nbt("nbt length overflow"))?;
        if end > self.bytes.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "nbt eof"));
        }
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn read_collection_len(&mut self, maximum: usize) -> io::Result<usize> {
        let value = self.read_i32()?;
        let value = usize::try_from(value).map_err(|_| invalid_nbt("negative nbt length"))?;
        if value > maximum {
            return Err(invalid_nbt("nbt collection exceeds maximum length"));
        }
        Ok(value)
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_i8(&mut self) -> io::Result<i8> {
        Ok(self.read_u8()? as i8)
    }

    fn read_u16(&mut self) -> io::Result<u16> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_i16(&mut self) -> io::Result<i16> {
        Ok(self.read_u16()? as i16)
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_i32(&mut self) -> io::Result<i32> {
        Ok(self.read_u32()? as i32)
    }

    fn read_u64(&mut self) -> io::Result<u64> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_i64(&mut self) -> io::Result<i64> {
        Ok(self.read_u64()? as i64)
    }
}

fn invalid_nbt(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_collection_lengths() {
        let bytes = [7, 0, 0, 0xff, 0xff, 0xff, 0xff];
        assert_eq!(
            parse_root(&bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn rejects_excessive_nesting() {
        let mut bytes = vec![9, 0, 0];
        for _ in 0..=MAX_NBT_DEPTH {
            bytes.push(9);
            bytes.extend_from_slice(&1i32.to_be_bytes());
        }
        bytes.push(1);
        bytes.extend_from_slice(&1i32.to_be_bytes());
        bytes.push(0);
        assert_eq!(
            parse_root(&bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
