use std::collections::HashMap;
use std::sync::Arc;

use super::block::Block;
use super::chunk::{Chunk, CHUNK_HEIGHT, CHUNK_SIZE};
use super::helpers::{distance_to_chunk, distance_to_interval};
use super::light::{self, ChunkLight};
use super::mesh::{self, ChunkMesh, MeshOptions};
use super::state::{
    MeshBuildResult, MeshChunkSnapshot, MeshJobToken, World, MAX_MESH_REMOVALS_PER_FRAME,
    MAX_MESH_RESULTS_PER_FRAME, MAX_NORMAL_BACKGROUND_MESH_JOBS, MAX_PRIORITY_BACKGROUND_MESH_JOBS,
};

impl World {
    pub fn poll_finished_meshes(&mut self) -> Vec<ChunkMesh> {
        let mut meshes = Vec::new();
        let mut finished_lights = Vec::new();
        let results = {
            let Ok(rx) = self.mesh_result_rx.lock() else {
                return meshes;
            };
            let mut results = Vec::with_capacity(MAX_MESH_RESULTS_PER_FRAME);
            for _ in 0..MAX_MESH_RESULTS_PER_FRAME {
                let Ok(result) = rx.try_recv() else {
                    break;
                };
                results.push(result);
            }
            results
        };
        for result in results {
            let mesh = result.mesh;
            let position = (mesh.cx, mesh.cz);
            let Some(inflight_token) = self.pending_mesh_inflight.get(&position).copied() else {
                continue;
            };
            if result.token != inflight_token {
                continue;
            }
            self.pending_mesh_inflight.remove(&position);
            let was_priority = self.pending_mesh_priority_inflight.remove(&position);
            if !self.chunks.contains_key(&position) {
                self.pending_mesh_dirty.remove(&position);
                self.pending_mesh_priority_dirty.remove(&position);
                continue;
            }
            let dirty = self.pending_mesh_dirty.remove(&position);
            let priority_dirty = self.pending_mesh_priority_dirty.remove(&position);
            if dirty || result.token != self.mesh_job_token(position) {
                if dirty && (was_priority || priority_dirty) {
                    self.pending_mesh_queue.retain(|queued| *queued != position);
                    if !self.pending_mesh_priority_queue.contains(&position) {
                        self.pending_mesh_priority_queue.push_back(position);
                    }
                } else if dirty
                    && !self.pending_mesh_queue.contains(&position)
                    && !self.pending_mesh_priority_queue.contains(&position)
                {
                    self.pending_mesh_queue.push_back(position);
                }
                continue;
            }
            if let Some(light) = result.light {
                finished_lights.push((position, light));
            }
            meshes.push(mesh);
        }
        for ((cx, cz), light) in finished_lights {
            self.light.insert_chunk(cx, cz, light);
            self.dirty_light_chunks.remove(&(cx, cz));
            self.mark_snapshot_chunk_changed(cx, cz);
        }
        meshes
    }

    pub fn schedule_background_meshes(&mut self) -> Vec<ChunkMesh> {
        let mut removals = Vec::new();
        for _ in 0..MAX_MESH_REMOVALS_PER_FRAME {
            let Some((cx, cz)) = self.pending_mesh_removals.pop_front() else {
                break;
            };
            removals.push(ChunkMesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                cx,
                cz,
                transparent_start: 0,
                aabb_min: [f32::MAX; 3],
                aabb_max: [f32::MIN; 3],
            });
        }

        while self.pending_mesh_priority_inflight.len() < MAX_PRIORITY_BACKGROUND_MESH_JOBS {
            let Some((cx, cz)) = self.pending_mesh_priority_queue.pop_front() else {
                break;
            };
            let position = (cx, cz);
            if !self.chunks.contains_key(&position)
                || self.pending_mesh_inflight.contains_key(&position)
            {
                continue;
            }
            self.spawn_background_mesh_job(cx, cz, true);
        }

        while self
            .pending_mesh_inflight
            .len()
            .saturating_sub(self.pending_mesh_priority_inflight.len())
            < MAX_NORMAL_BACKGROUND_MESH_JOBS
        {
            let Some((cx, cz)) = self.pending_mesh_queue.pop_front() else {
                break;
            };
            let position = (cx, cz);
            if !self.chunks.contains_key(&position)
                || self.pending_mesh_inflight.contains_key(&position)
            {
                continue;
            }
            self.spawn_background_mesh_job(cx, cz, false);
        }

        removals
    }

    pub(crate) fn spawn_background_mesh_job(&mut self, cx: i32, cz: i32, priority: bool) {
        let position = (cx, cz);
        let token = self.mesh_job_token(position);
        self.pending_mesh_inflight.insert(position, token);
        if priority {
            self.pending_mesh_priority_inflight.insert(position);
        }
        let snapshot = self.mesh_snapshot(cx, cz);
        let options = self.mesh_options;
        let tx = self.mesh_result_tx.clone();
        rayon::spawn(move || {
            let result = build_mesh_from_snapshot(cx, cz, snapshot, options, token);
            let _ = tx.send(result);
        });
    }

    pub(crate) fn mesh_snapshot(&self, cx: i32, cz: i32) -> HashMap<(i32, i32), MeshChunkSnapshot> {
        let mut snapshot = HashMap::with_capacity(9);
        for dx in -1..=1 {
            for dz in -1..=1 {
                let (Some(snapshot_cx), Some(snapshot_cz)) =
                    (cx.checked_add(dx), cz.checked_add(dz))
                else {
                    continue;
                };
                let coord = (snapshot_cx, snapshot_cz);
                if let Some(chunk) = self.chunks.get(&coord) {
                    let local_light = self.light.shared_chunk(coord.0, coord.1);
                    let network_light_valid =
                        chunk.has_valid_network_light() && local_light.is_none();
                    let light_dirty = self.dirty_light_chunks.contains(&coord);
                    snapshot.insert(
                        coord,
                        MeshChunkSnapshot {
                            chunk: chunk.clone(),
                            network_light_valid,
                            local_light,
                            light_dirty,
                        },
                    );
                }
            }
        }
        snapshot
    }

    /// Build up to `budget` chunk meshes from the pending queue.
    /// Returns the built meshes for GPU upload.
    pub fn build_pending_meshes(&mut self, budget: usize) -> Vec<ChunkMesh> {
        let mut meshes = Vec::new();
        for _ in 0..budget {
            if let Some((cx, cz)) = self.pending_mesh_removals.pop_front() {
                meshes.push(ChunkMesh {
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    cx,
                    cz,
                    transparent_start: 0,
                    aabb_min: [f32::MAX; 3],
                    aabb_max: [f32::MIN; 3],
                });
                continue;
            }
            let next = self
                .pending_mesh_priority_queue
                .pop_front()
                .or_else(|| self.pending_mesh_queue.pop_front());
            let Some((cx, cz)) = next else {
                break;
            };
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                // Compute light before building mesh (deferred from network path)
                self.light.compute_chunk(chunk);
                let mesh = mesh::build_chunk_mesh(
                    chunk,
                    |wx, wy, wz| self.get_block(wx, wy, wz),
                    |wx, wy, wz| self.light_at(wx, wy, wz),
                    |wx, wy, wz| self.get_block_state(wx, wy, wz),
                    self.mesh_options,
                );
                // Deferred light computation can change the values exposed by
                // the scripting snapshot even though block data is stable.
                self.mark_snapshot_chunk_changed(cx, cz);
                if mesh.vertices.is_empty() {
                    log::debug!("chunk mesh is empty: chunk=({cx},{cz})");
                }
                meshes.push(mesh);
            }
        }
        meshes
    }
}

pub(crate) fn build_mesh_from_snapshot(
    cx: i32,
    cz: i32,
    chunks: HashMap<(i32, i32), MeshChunkSnapshot>,
    options: MeshOptions,
    token: MeshJobToken,
) -> MeshBuildResult {
    let Some(center) = chunks.get(&(cx, cz)) else {
        return MeshBuildResult {
            token,
            mesh: ChunkMesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                cx,
                cz,
                transparent_start: 0,
                aabb_min: [f32::MAX; 3],
                aabb_max: [f32::MIN; 3],
            },
            light: None,
        };
    };

    // Meshes are generated off-thread, so they cannot borrow World::light.
    // Recreate the same source used by the synchronous path from this chunk
    // snapshot: server nibbles first, then local propagation for invalidated
    // chunks. This prevents a changed chunk from falling back to sky=15 and
    // emission-only block light.
    let chunk = center.chunk.as_ref();
    let center_light_needs_rebuild =
        center.light_dirty || (!center.network_light_valid && center.local_light.is_none());
    let mut rebuild_light = chunks
        .iter()
        .filter_map(|(&position, source)| {
            (source.light_dirty || (!source.network_light_valid && source.local_light.is_none()))
                .then_some(position)
        })
        .collect::<Vec<_>>();
    let mut snapshot_light = light::WorldLight::new();
    if !rebuild_light.is_empty() {
        // Seed every chunk with its last complete snapshot before replacing
        // dirty entries. Cross-chunk propagation therefore never sees an
        // artificial zero-light hole while adjacent chunks are rebuilt.
        for (&position, source) in &chunks {
            if let Some(local) = &source.local_light {
                snapshot_light.insert_chunk(position.0, position.1, local.clone());
            } else {
                snapshot_light.compute_chunk_with_validity(&source.chunk, true);
            }
        }

        // A light level reaches at most 15 blocks while a chunk is 16 wide.
        // Two deterministic sweeps across this 3x3 snapshot cover both paths:
        // outward from the target and inward from neighboring/diagonal sources.
        // This avoids HashMap iteration order changing cross-chunk lighting.
        rebuild_light.sort_unstable_by_key(|&(lcx, lcz)| {
            (
                (i64::from(lcx) - i64::from(cx)).abs() + (i64::from(lcz) - i64::from(cz)).abs(),
                lcx,
                lcz,
            )
        });
        for &(lcx, lcz) in &rebuild_light {
            snapshot_light.compute_chunk_with_validity(&chunks[&(lcx, lcz)].chunk, false);
        }
        for &(lcx, lcz) in rebuild_light.iter().rev() {
            snapshot_light.compute_chunk_with_validity(&chunks[&(lcx, lcz)].chunk, false);
        }
    }

    let get_chunk = |wx: i32, wz: i32| {
        let ncx = wx.div_euclid(CHUNK_SIZE as i32);
        let ncz = wz.div_euclid(CHUNK_SIZE as i32);
        chunks.get(&(ncx, ncz))
    };
    let local = |wx: i32, wz: i32| {
        (
            wx.rem_euclid(CHUNK_SIZE as i32) as usize,
            wz.rem_euclid(CHUNK_SIZE as i32) as usize,
        )
    };

    let mesh = mesh::build_chunk_mesh(
        chunk,
        |wx, wy, wz| {
            if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
                return Block::Air;
            }
            let (lx, lz) = local(wx, wz);
            get_chunk(wx, wz).map_or(Block::Air, |source| source.chunk.get(lx, wy as usize, lz))
        },
        |wx, wy, wz| {
            if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
                return light::LightLevel { sky: 0, block: 0 };
            }
            let (lx, lz) = local(wx, wz);
            let ncx = wx.div_euclid(CHUNK_SIZE as i32);
            let ncz = wz.div_euclid(CHUNK_SIZE as i32);
            get_chunk(wx, wz).map_or(light::LightLevel { sky: 0, block: 0 }, |source| {
                if source.light_dirty
                    || (!source.network_light_valid && source.local_light.is_none())
                {
                    snapshot_light.get(ncx, ncz, lx, wy as usize, lz)
                } else if let Some(local) = &source.local_light {
                    local.get(lx, wy as usize, lz)
                } else {
                    let (sky, block) = source.chunk.light_at(lx, wy as usize, lz);
                    light::LightLevel { sky, block }
                }
            })
        },
        |wx, wy, wz| {
            if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
                return 0;
            }
            let (lx, lz) = local(wx, wz);
            get_chunk(wx, wz).map_or(0, |source| source.chunk.state(lx, wy as usize, lz))
        },
        options,
    );
    let light = center_light_needs_rebuild
        .then(|| snapshot_light.take_chunk(cx, cz))
        .flatten();
    MeshBuildResult { token, mesh, light }
}
