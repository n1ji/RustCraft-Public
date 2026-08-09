use super::block::Block;
use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE};
use super::state::World;

impl World {
    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> Block {
        if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
            return Block::Air;
        }
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        let lx = wx.rem_euclid(CHUNK_SIZE as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_SIZE as i32) as usize;
        match self.chunks.get(&(cx, cz)) {
            Some(chunk) => chunk.get(lx, wy as usize, lz),
            None => Block::Air,
        }
    }

    /// Equivalent to 1.8.9 `Block.isNormalCube()` for the block-state model
    /// represented by this client. Normal cubes are opaque full blocks; all
    /// blocks with state-dependent or non-full shapes are excluded.
    pub fn is_normal_cube(&self, wx: i32, wy: i32, wz: i32) -> bool {
        let block = self.get_block(wx, wy, wz);
        block.properties().is_opaque
            && !crate::world::shape::has_custom_shape(block)
            // BlockCompressedPowered (the redstone block) is a full opaque
            // cube but overrides canProvidePower, so Block.isNormalCube is
            // false. Other vanilla power providers are custom-shape blocks.
            && block != Block::RedstoneBlock
    }

    pub fn get_block_state(&self, wx: i32, wy: i32, wz: i32) -> u16 {
        if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
            return 0;
        }
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        let lx = wx.rem_euclid(CHUNK_SIZE as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_SIZE as i32) as usize;
        self.chunks
            .get(&(cx, cz))
            .map_or(0, |chunk| chunk.state(lx, wy as usize, lz))
    }

    pub fn get_block_metadata(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        (self.get_block_state(wx, wy, wz) & 0x0f) as u8
    }

    /// Check if the eye position is underwater (inside a water block).
    pub fn is_water_at(&self, x: f32, y: f32, z: f32) -> bool {
        let wx = x.floor() as i32;
        let wy = y.floor() as i32;
        let wz = z.floor() as i32;
        matches!(
            self.get_block(wx, wy, wz),
            Block::FlowingWater | Block::StillWater
        )
    }
}
