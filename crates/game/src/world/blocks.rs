use std::sync::Arc;

use super::block::Block;
use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE};
use super::entities::is_chest_block;
use super::helpers::block_change_affects_light;
use super::state::World;

impl World {
    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: Block) -> bool {
        self.set_block_state(wx, wy, wz, block.to_id() << 4)
    }

    /// Mirrors vanilla `Chunk#setBlockState`: an identical complete state is a
    /// no-op, and cached light is invalidated only when opacity/emission changes.
    pub fn set_block_state(&mut self, wx: i32, wy: i32, wz: i32, block_state: u16) -> bool {
        if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
            return false;
        }
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        let lx = wx.rem_euclid(CHUNK_SIZE as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_SIZE as i32) as usize;
        let Some(previous_state) = self
            .chunks
            .get(&(cx, cz))
            .map(|chunk| chunk.state(lx, wy as usize, lz))
        else {
            return false;
        };
        if previous_state == block_state {
            return false;
        }
        if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
            let chunk = Arc::make_mut(chunk);
            let previous_block = Block::from_state(previous_state);
            let next_block = Block::from_state(block_state);
            chunk.set_state(lx, wy as usize, lz, block_state);
            if block_change_affects_light(previous_block, next_block) {
                // Vanilla updates its light nibbles through World#checkLight.
                // Our received nibbles are immutable snapshots, so retain the
                // bytes as a propagation baseline but stop treating them as
                // authoritative after opacity or emission changes.
                chunk.invalidate_network_light();
            }
            self.mark_snapshot_block_changed(wx, wy, wz);
        }
        self.invalidate_chunk_mesh((cx, cz));
        if Block::from_state(block_state) != Block::Skull {
            self.skulls.remove(&(wx, wy, wz));
        }
        if is_chest_block(Block::from_state(block_state)) {
            self.chests.entry((wx, wy, wz)).or_default();
        } else {
            self.chests.remove(&(wx, wy, wz));
        }
        if is_chest_block(Block::from_state(previous_state))
            || is_chest_block(Block::from_state(block_state))
        {
            self.chest_structure_generation = self.chest_structure_generation.wrapping_add(1);
        }
        true
    }
}
