use std::collections::HashSet;
use std::sync::Arc;

use super::block::Block;
use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE};
use super::helpers::{
    block_change_affects_light, chunks_affected_by_light_change, distance_to_chunk,
};
use super::{light, state::World};

impl World {
    pub fn set_block_and_relight(&mut self, wx: i32, wy: i32, wz: i32, block: Block) -> bool {
        self.set_block_state_and_relight(wx, wy, wz, block.to_id() << 4)
    }

    pub fn set_block_state_and_relight(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        block_state: u16,
    ) -> bool {
        let previous_state = self.get_block_state(wx, wy, wz);
        if !self.set_block_state(wx, wy, wz, block_state) {
            return false;
        }
        if block_change_affects_light(
            Block::from_state(previous_state),
            Block::from_state(block_state),
        ) {
            self.update_light_around_positions(&[(wx, wy, wz)]);
        }
        true
    }

    pub fn recompute_all_light(&mut self) {
        for chunk in self.chunks.values() {
            self.light.compute_chunk(chunk);
        }
        self.dirty_light_chunks.clear();
        if !self.chunks.is_empty() {
            self.mark_snapshot_changed();
        }
    }

    pub fn recompute_chunk_light(&mut self, cx: i32, cz: i32) {
        if self.chunks.contains_key(&(cx, cz)) {
            let chunk = self
                .chunks
                .get(&(cx, cz))
                .expect("chunk existence checked above");
            self.light.compute_chunk(chunk);
            self.dirty_light_chunks.remove(&(cx, cz));
            self.invalidate_chunk_mesh((cx, cz));
            self.mark_snapshot_chunk_changed(cx, cz);
        }
    }

    pub(crate) fn update_light_around_positions(&mut self, positions: &[(i32, i32, i32)]) {
        let mut affected_chunks = positions
            .iter()
            .flat_map(|&(wx, _, wz)| chunks_affected_by_light_change(wx, wz))
            .filter(|position| self.chunks.contains_key(position))
            .collect::<Vec<_>>();
        affected_chunks.sort_unstable();
        affected_chunks.dedup();

        // A light source can affect neighboring chunks even when the changed
        // block is not on a chunk edge. Drop stale local propagation and mark
        // every reachable server snapshot non-authoritative before updating.
        for &(cx, cz) in &affected_chunks {
            self.light.remove_chunk(cx, cz);
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                chunk.invalidate_network_light();
            }
        }

        let mut changed_chunks = HashSet::new();
        for &position in positions {
            changed_chunks.extend(self.light.update_around(&self.chunks, position));
        }

        // World#checkLight refuses an incremental update when its radius is not
        // loaded. Local placement still needs immediate feedback, so rebuild
        // the loaded affected region from block states in that case.
        if changed_chunks.is_empty() {
            affected_chunks.sort_by_key(|&(cx, cz)| {
                positions
                    .iter()
                    .map(|&(wx, _, wz)| distance_to_chunk(wx, wz, cx, cz))
                    .min()
                    .unwrap_or(i64::MAX)
            });
            for &(cx, cz) in &affected_chunks {
                if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                    self.light.compute_chunk(chunk);
                    changed_chunks.insert((cx, cz));
                }
            }
        }

        let mut changed_chunks = changed_chunks.into_iter().collect::<Vec<_>>();
        changed_chunks.sort_unstable();
        for position in &affected_chunks {
            self.dirty_light_chunks.remove(position);
        }
        for &(cx, cz) in &changed_chunks {
            // Light can cross a chunk edge, so every chunk whose stored light
            // changed needs a mesh refresh. These remain normal-lane jobs.
            self.enqueue_chunk_mesh(cx, cz);
        }
        self.mark_snapshot_chunks_changed(&changed_chunks);
    }

    /// Network block changes must not perform radius-wide propagation on the
    /// main thread. Invalidate the affected snapshots and let their queued mesh
    /// jobs compute light from the captured block states.
    pub(crate) fn invalidate_light_around_positions(&mut self, positions: &[(i32, i32, i32)]) {
        let mut affected_chunks = positions
            .iter()
            .flat_map(|&(wx, _, wz)| chunks_affected_by_light_change(wx, wz))
            .filter(|position| self.chunks.contains_key(position))
            .collect::<Vec<_>>();
        affected_chunks.sort_unstable();
        affected_chunks.dedup();
        for &(cx, cz) in &affected_chunks {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                // Keep one complete baseline visible until the background job
                // atomically publishes its replacement. Dropping this cache
                // used to make entities and neighboring meshes sample (0, 0).
                self.light.ensure_network_baseline(chunk);
                chunk.invalidate_network_light();
                self.dirty_light_chunks.insert((cx, cz));
            }
            self.enqueue_chunk_mesh(cx, cz);
        }
        self.mark_snapshot_chunks_changed(&affected_chunks);
    }

    pub(crate) fn light_at(&self, wx: i32, wy: i32, wz: i32) -> light::LightLevel {
        if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
            return light::LightLevel { sky: 15, block: 0 };
        }
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        let lx = wx.rem_euclid(CHUNK_SIZE as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_SIZE as i32) as usize;
        if let Some(local) = self.light.chunk(cx, cz) {
            return local.get(lx, wy as usize, lz);
        }
        if let Some(chunk) = self.chunks.get(&(cx, cz)) {
            // The raw nibbles remain the last complete server snapshot even
            // after a local edit invalidates their authority. They are a much
            // safer fallback than exposing a temporary black light value.
            let (sky, block) = chunk.light_at(lx, wy as usize, lz);
            return light::LightLevel { sky, block };
        }
        self.light.get(cx, cz, lx, wy as usize, lz)
    }

    /// Returns the lightmap value used by block and entity rendering at a world position.
    pub fn light_at_world(&self, wx: i32, wy: i32, wz: i32) -> light::LightLevel {
        self.light_at(wx, wy, wz)
    }

    pub fn set_smooth_lighting(&mut self, enabled: bool) {
        self.mesh_options.smooth_lighting = enabled;
    }

    pub fn smooth_lighting(&self) -> bool {
        self.mesh_options.smooth_lighting
    }

    pub fn set_sky_brightness(&mut self, brightness: f32) {
        self.mesh_options.sky_brightness = brightness;
    }
}
