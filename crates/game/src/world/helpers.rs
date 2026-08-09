use std::collections::HashMap;

use super::block::Block;
use super::chunk::CHUNK_SIZE;
use super::light;

pub(crate) fn nbt_u8(map: &HashMap<String, crate::net::nbt::NbtTag>, key: &str) -> Option<u8> {
    map.get(key)
        .and_then(crate::net::nbt::NbtTag::as_i32)
        .map(|value| value.clamp(0, u8::MAX as i32) as u8)
}

pub(crate) fn normalize_uuid(value: &str) -> String {
    let compact: String = value.chars().filter(|c| *c != '-').collect();
    if compact.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &compact[0..8],
            &compact[8..12],
            &compact[12..16],
            &compact[16..20],
            &compact[20..32]
        )
    } else {
        value.to_owned()
    }
}

pub(crate) fn chunk_and_border_neighbors(wx: i32, wz: i32) -> Vec<(i32, i32)> {
    let cx = wx.div_euclid(CHUNK_SIZE as i32);
    let cz = wz.div_euclid(CHUNK_SIZE as i32);
    let lx = wx.rem_euclid(CHUNK_SIZE as i32);
    let lz = wz.rem_euclid(CHUNK_SIZE as i32);
    let mut out = Vec::with_capacity(4);
    if lx == 0 {
        out.push((cx - 1, cz));
    }
    if lx == CHUNK_SIZE as i32 - 1 {
        out.push((cx + 1, cz));
    }
    if lz == 0 {
        out.push((cx, cz - 1));
    }
    if lz == CHUNK_SIZE as i32 - 1 {
        out.push((cx, cz + 1));
    }
    out
}

pub(crate) fn chunks_affected_by_light_change(wx: i32, wz: i32) -> Vec<(i32, i32)> {
    const RADIUS: i64 = 15;
    let wx = i64::from(wx);
    let wz = i64::from(wz);
    let min_cx = (wx - RADIUS).div_euclid(CHUNK_SIZE as i64);
    let max_cx = (wx + RADIUS).div_euclid(CHUNK_SIZE as i64);
    let min_cz = (wz - RADIUS).div_euclid(CHUNK_SIZE as i64);
    let max_cz = (wz + RADIUS).div_euclid(CHUNK_SIZE as i64);
    let mut chunks = Vec::with_capacity(9);
    for cx in min_cx..=max_cx {
        for cz in min_cz..=max_cz {
            if distance_to_chunk(wx as i32, wz as i32, cx as i32, cz as i32) <= RADIUS {
                chunks.push((cx as i32, cz as i32));
            }
        }
    }
    chunks
}

pub(crate) fn distance_to_chunk(wx: i32, wz: i32, cx: i32, cz: i32) -> i64 {
    let chunk_min_x = i64::from(cx) * CHUNK_SIZE as i64;
    let chunk_min_z = i64::from(cz) * CHUNK_SIZE as i64;
    let chunk_max_x = chunk_min_x + CHUNK_SIZE as i64 - 1;
    let chunk_max_z = chunk_min_z + CHUNK_SIZE as i64 - 1;
    distance_to_interval(i64::from(wx), chunk_min_x, chunk_max_x)
        + distance_to_interval(i64::from(wz), chunk_min_z, chunk_max_z)
}

pub(crate) fn distance_to_interval(value: i64, min: i64, max: i64) -> i64 {
    if value < min {
        min - value
    } else if value > max {
        value - max
    } else {
        0
    }
}

#[inline]
pub(crate) fn block_change_affects_light(previous: Block, next: Block) -> bool {
    light::light_opacity(previous) != light::light_opacity(next)
        || light::block_light_emission(previous) != light::block_light_emission(next)
}
