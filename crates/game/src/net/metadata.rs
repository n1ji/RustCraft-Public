use super::protocol::PacketBuffer;
use super::slot::{self, Slot};
use std::io;

#[derive(Clone, Debug)]
pub enum MetadataValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(String),
    Slot(Slot),
    Position([i32; 3]),
    Rotation([f32; 3]),
}

#[derive(Clone, Debug)]
pub struct EntityMetadata {
    pub index: u8,
    pub value: MetadataValue,
}

pub fn read_entity_metadata(buf: &mut PacketBuffer) -> io::Result<Vec<EntityMetadata>> {
    let mut entries = Vec::new();
    let mut terminated = false;
    while buf.remaining() > 0 {
        let item = buf.read_unsigned_byte()?;
        if item == 0x7f {
            terminated = true;
            break;
        }
        if entries.len() >= 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "too many entity metadata entries",
            ));
        }

        let ty = item >> 5;
        let index = item & 0x1f;
        let value = match ty {
            0 => MetadataValue::Byte(buf.read_byte()?),
            1 => MetadataValue::Short(buf.read_short()?),
            2 => MetadataValue::Int(buf.read_int()?),
            3 => MetadataValue::Float(buf.read_float()?),
            4 => MetadataValue::String(buf.read_string()?),
            5 => MetadataValue::Slot(slot::read_slot(buf)?),
            6 => {
                let x = buf.read_int()?;
                let y = buf.read_int()?;
                let z = buf.read_int()?;
                MetadataValue::Position([x, y, z])
            }
            7 => {
                let x = buf.read_float()?;
                let y = buf.read_float()?;
                let z = buf.read_float()?;
                MetadataValue::Rotation([x, y, z])
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid entity metadata type {}", ty),
                ))
            }
        };
        entries.push(EntityMetadata { index, value });
    }
    if !terminated {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "entity metadata is missing its terminator",
        ));
    }
    Ok(entries)
}
