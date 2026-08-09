use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE};
use super::helpers::chunk_and_border_neighbors;
use super::light;
use super::mesh::{self, ChunkMesh};
use super::state::World;

impl World {
    pub fn build_all_meshes(&self) -> Vec<ChunkMesh> {
        use rayon::prelude::*;
        self.chunks
            .par_iter()
            .map(|((_cx, _cz), chunk)| {
                mesh::build_chunk_mesh(
                    chunk,
                    |wx, wy, wz| self.get_block(wx, wy, wz),
                    |wx, wy, wz| self.light_at(wx, wy, wz),
                    |wx, wy, wz| self.get_block_state(wx, wy, wz),
                    self.mesh_options,
                )
            })
            .filter(|m| !m.is_empty())
            .collect()
    }

    /// Rebuild only the mesh for a specific chunk (and neighbors if on border).
    /// Returns list of updated meshes.
    pub fn rebuild_chunk_at(&mut self, wx: i32, _wy: i32, wz: i32) -> Vec<ChunkMesh> {
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);

        let mut to_rebuild = vec![(cx, cz)];
        to_rebuild.extend(chunk_and_border_neighbors(wx, wz));
        to_rebuild.sort_unstable();
        to_rebuild.dedup();

        let mut meshes = Vec::new();
        for (rcx, rcz) in to_rebuild {
            let position = (rcx, rcz);
            if !self.chunks.contains_key(&position) {
                continue;
            }
            // This synchronous mesh is built from the latest world state. Any
            // queued work is now redundant, while an already-running job must
            // remain tracked until its tokened result arrives and is rejected.
            self.invalidate_chunk_mesh(position);
            self.pending_mesh_queue.retain(|queued| *queued != position);
            self.pending_mesh_priority_queue
                .retain(|queued| *queued != position);
            self.pending_mesh_dirty.remove(&position);
            self.pending_mesh_priority_dirty.remove(&position);
            if let Some(chunk) = self.chunks.get(&(rcx, rcz)) {
                let mesh = mesh::build_chunk_mesh(
                    chunk,
                    |wx, wy, wz| self.get_block(wx, wy, wz),
                    |wx, wy, wz| self.light_at(wx, wy, wz),
                    |wx, wy, wz| self.get_block_state(wx, wy, wz),
                    self.mesh_options,
                );
                if !mesh.is_empty() {
                    meshes.push(mesh);
                }
            }
        }
        meshes
    }

    /// Build the changed chunk immediately for local placement prediction.
    /// This deliberately keeps the queued background job: the provisional
    /// mesh uses the last server light nibbles so it can be displayed now,
    /// then the background result replaces it with recomputed lighting.
    pub fn build_immediate_mesh_at_block(&self, wx: i32, wz: i32) -> Option<ChunkMesh> {
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        let chunk = self.chunks.get(&(cx, cz))?;
        let provisional_light = |lx: i32, ly: i32, lz: i32| {
            if ly < 0 || ly >= CHUNK_HEIGHT as i32 {
                return light::LightLevel { sky: 15, block: 0 };
            }
            let light_cx = lx.div_euclid(CHUNK_SIZE as i32);
            let light_cz = lz.div_euclid(CHUNK_SIZE as i32);
            let local_x = lx.rem_euclid(CHUNK_SIZE as i32) as usize;
            let local_z = lz.rem_euclid(CHUNK_SIZE as i32) as usize;
            if let Some(local) = self.light.chunk(light_cx, light_cz) {
                return local.get(local_x, ly as usize, local_z);
            }
            if let Some(light_chunk) = self.chunks.get(&(light_cx, light_cz)) {
                let (sky, block) = light_chunk.light_at(local_x, ly as usize, local_z);
                light::LightLevel { sky, block }
            } else {
                self.light
                    .get(light_cx, light_cz, local_x, ly as usize, local_z)
            }
        };
        Some(mesh::build_chunk_mesh(
            chunk,
            |x, y, z| self.get_block(x, y, z),
            provisional_light,
            |x, y, z| self.get_block_state(x, y, z),
            self.mesh_options,
        ))
    }
}
