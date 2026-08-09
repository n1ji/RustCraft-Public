//! std140 layout computation for patcher-generated `FrameUniforms` blocks.
//!
//! The GLSL patcher hoists every non-opaque uniform of a shader pack program
//! into one uniform block whose field order is program-specific, so the CPU
//! side cannot use a fixed struct. This module recomputes the std140 offsets
//! from the patcher's declaration strings (`"mat4 gbufferModelView"`,
//! `"vec3 offsets[9]"`, ...) and writes named values into a byte buffer.

/// Scalar/vector/matrix classes distinguished by std140 rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Scalar,
    Vec2,
    Vec3,
    Vec4,
    Mat3,
    Mat4,
}

impl FieldType {
    fn parse(type_name: &str) -> Option<Self> {
        Some(match type_name {
            "float" | "int" | "uint" | "bool" => Self::Scalar,
            "vec2" | "ivec2" | "uvec2" | "bvec2" => Self::Vec2,
            "vec3" | "ivec3" | "uvec3" | "bvec3" => Self::Vec3,
            "vec4" | "ivec4" | "uvec4" | "bvec4" => Self::Vec4,
            "mat3" => Self::Mat3,
            "mat4" => Self::Mat4,
            _ => return None,
        })
    }

    /// (size, alignment) of a non-array field.
    fn layout(self) -> (u32, u32) {
        match self {
            Self::Scalar => (4, 4),
            Self::Vec2 => (8, 8),
            Self::Vec3 => (12, 16),
            Self::Vec4 => (16, 16),
            // mat3 lays out as 3 columns padded to vec4 stride.
            Self::Mat3 => (48, 16),
            Self::Mat4 => (64, 16),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
    /// Non-zero for arrays. std140 pads every element stride to 16 bytes.
    pub array_len: u32,
    pub offset: u32,
}

/// A value that can be written into a field; wider sources are truncated and
/// narrower ones zero-padded, so callers can keep one canonical value per
/// uniform name (e.g. always `Vec4` for positions).
#[derive(Clone, Copy, Debug)]
pub enum Value {
    Float(f32),
    Int(i32),
    Uint(u32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    IVec2([i32; 2]),
    Mat4([[f32; 4]; 4]),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub fields: Vec<Field>,
    pub size: u32,
}

fn align_up(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

impl Layout {
    /// Compute offsets for the patcher's field declarations, in order.
    /// Unknown types are treated as vec4 so later offsets stay conservative
    /// rather than corrupting every following field.
    pub fn from_decls(decls: &[String]) -> Self {
        let mut fields = Vec::with_capacity(decls.len());
        let mut cursor = 0u32;
        for decl in decls {
            let mut parts = decl.split_whitespace();
            let type_name = parts.next().unwrap_or("");
            let rest: String = parts.collect::<Vec<_>>().join(" ");
            let name = rest.split('[').next().unwrap_or("").trim().to_string();
            let array_len = rest
                .split_once('[')
                .and_then(|(_, tail)| tail.split(']').next())
                .and_then(|len| len.trim().parse::<u32>().ok())
                .unwrap_or(0);
            let ty = FieldType::parse(type_name).unwrap_or_else(|| {
                log::warn!("std140: unknown uniform type '{type_name}', assuming vec4");
                FieldType::Vec4
            });
            let (size, mut align) = ty.layout();
            let stride;
            if array_len > 0 {
                align = align.max(16);
                stride = align_up(size, 16);
            } else {
                stride = size;
            }
            cursor = align_up(cursor, align);
            fields.push(Field {
                name,
                ty,
                array_len,
                offset: cursor,
            });
            cursor += stride * array_len.max(1);
        }
        Self {
            size: align_up(cursor.max(4), 16),
            fields,
        }
    }

    /// Write `value` into the field called `name`; returns false when the
    /// block has no such field. Arrays receive the value in element 0 only.
    pub fn write(&self, buffer: &mut [u8], name: &str, value: Value) -> bool {
        let Some(field) = self.fields.iter().find(|field| field.name == name) else {
            return false;
        };
        let floats: [f32; 4];
        let components = match value {
            Value::Float(v) => {
                floats = [v, 0.0, 0.0, 0.0];
                &floats[..1]
            }
            Value::Int(v) => {
                return self.write_ints(buffer, field, &[v]);
            }
            Value::Uint(v) => {
                return self.write_ints(buffer, field, &[v as i32]);
            }
            Value::IVec2(v) => {
                return self.write_ints(buffer, field, &v);
            }
            Value::Vec2(v) => {
                floats = [v[0], v[1], 0.0, 0.0];
                &floats[..2]
            }
            Value::Vec3(v) => {
                floats = [v[0], v[1], v[2], 0.0];
                &floats[..3]
            }
            Value::Vec4(v) => {
                floats = v;
                &floats[..4]
            }
            Value::Mat4(m) => {
                return self.write_matrix(buffer, field, &m);
            }
        };
        self.write_floats(buffer, field, components);
        true
    }

    fn field_component_count(field: &Field) -> usize {
        match field.ty {
            FieldType::Scalar => 1,
            FieldType::Vec2 => 2,
            FieldType::Vec3 => 3,
            FieldType::Vec4 => 4,
            FieldType::Mat3 => 12,
            FieldType::Mat4 => 16,
        }
    }

    fn write_floats(&self, buffer: &mut [u8], field: &Field, components: &[f32]) {
        let count = Self::field_component_count(field).min(4);
        let offset = field.offset as usize;
        for index in 0..count {
            let value = components.get(index).copied().unwrap_or(0.0);
            let at = offset + index * 4;
            if at + 4 <= buffer.len() {
                buffer[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
    }

    fn write_ints(&self, buffer: &mut [u8], field: &Field, components: &[i32]) -> bool {
        let count = Self::field_component_count(field).min(4);
        let offset = field.offset as usize;
        for index in 0..count {
            let value = components.get(index).copied().unwrap_or(0);
            let at = offset + index * 4;
            if at + 4 <= buffer.len() {
                buffer[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        true
    }

    /// Matrices are written column-major. A mat3 field takes the upper-left
    /// 3x3 of the source with vec4 column stride per std140.
    fn write_matrix(&self, buffer: &mut [u8], field: &Field, m: &[[f32; 4]; 4]) -> bool {
        let offset = field.offset as usize;
        let (columns, rows, stride) = match field.ty {
            FieldType::Mat4 => (4usize, 4usize, 16usize),
            FieldType::Mat3 => (3, 3, 16),
            // Writing a matrix into a smaller field: take the first column.
            _ => {
                self.write_floats(buffer, field, &m[0]);
                return true;
            }
        };
        for column in 0..columns {
            for row in 0..rows {
                let at = offset + column * stride + row * 4;
                if at + 4 <= buffer.len() {
                    buffer[at..at + 4].copy_from_slice(&m[column][row].to_le_bytes());
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn scalar_packing_matches_std140() {
        let layout = Layout::from_decls(&decls(&["float a", "vec3 b", "float c", "vec2 d"]));
        let offsets: Vec<u32> = layout.fields.iter().map(|f| f.offset).collect();
        // float @0, vec3 aligns to 16, float fits right after vec3 (@28),
        // vec2 aligns to 8 (@32).
        assert_eq!(offsets, vec![0, 16, 28, 32]);
        assert_eq!(layout.size, 48);
    }

    #[test]
    fn matrices_and_arrays() {
        let layout = Layout::from_decls(&decls(&["mat4 m", "vec2 pad", "vec3 arr[3]", "int i"]));
        let offsets: Vec<u32> = layout.fields.iter().map(|f| f.offset).collect();
        // mat4 @0 (64), vec2 @64, arr aligns to 16 @80 with 16-byte stride
        // (3*16=48), int @128.
        assert_eq!(offsets, vec![0, 64, 80, 128]);
    }

    #[test]
    fn writes_values_at_field_offsets() {
        let layout = Layout::from_decls(&decls(&["float a", "vec3 b"]));
        let mut buffer = vec![0u8; layout.size as usize];
        assert!(layout.write(&mut buffer, "a", Value::Float(2.5)));
        assert!(layout.write(&mut buffer, "b", Value::Vec4([1.0, 2.0, 3.0, 9.0])));
        assert!(!layout.write(&mut buffer, "missing", Value::Float(0.0)));
        assert_eq!(f32::from_le_bytes(buffer[0..4].try_into().unwrap()), 2.5);
        assert_eq!(f32::from_le_bytes(buffer[16..20].try_into().unwrap()), 1.0);
        assert_eq!(f32::from_le_bytes(buffer[24..28].try_into().unwrap()), 3.0);
        // vec3 field truncates the vec4 source.
        assert_eq!(f32::from_le_bytes(buffer[28..32].try_into().unwrap()), 0.0);
    }

    #[test]
    fn int_written_raw() {
        let layout = Layout::from_decls(&decls(&["int worldTime"]));
        let mut buffer = vec![0u8; layout.size as usize];
        assert!(layout.write(&mut buffer, "worldTime", Value::Int(-3)));
        assert_eq!(i32::from_le_bytes(buffer[0..4].try_into().unwrap()), -3);
    }
}
