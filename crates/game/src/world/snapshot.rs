use super::chunk::CHUNK_SIZE;
use super::state::{MeshJobToken, SnapshotChanges, SnapshotRegionRevision, World};

impl World {
    /// Changes whenever a cached scripting block snapshot may have gone stale.
    pub fn snapshot_revision(&self) -> u64 {
        self.snapshot_revision
    }

    /// Exact generation fingerprint for the chunks intersecting a block cube.
    /// A distant server update therefore does not invalidate the Lua block cache.
    pub fn snapshot_revision_around(
        &self,
        center: (i32, i32, i32),
        radius: i32,
    ) -> SnapshotRegionRevision {
        const MAX_EXACT_CHUNKS: i64 = 4_096;
        let radius = i64::from(radius.max(0));
        let min_cx = (i64::from(center.0) - radius).div_euclid(CHUNK_SIZE as i64);
        let max_cx = (i64::from(center.0) + radius).div_euclid(CHUNK_SIZE as i64);
        let min_cz = (i64::from(center.2) - radius).div_euclid(CHUNK_SIZE as i64);
        let max_cz = (i64::from(center.2) + radius).div_euclid(CHUNK_SIZE as i64);
        let chunk_count = (max_cx - min_cx + 1).saturating_mul(max_cz - min_cz + 1);
        if chunk_count > MAX_EXACT_CHUNKS {
            // This API normally receives the scripting radius (15). Fall back
            // to the global generation for pathological callers rather than
            // allocating or iterating an attacker-sized rectangle.
            return SnapshotRegionRevision {
                epoch: self.snapshot_revision,
                chunks: Vec::new(),
            };
        }
        let mut chunks = Vec::with_capacity(chunk_count as usize);
        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                let position = (cx as i32, cz as i32);
                chunks.push((
                    position,
                    self.snapshot_chunk_revisions
                        .get(&position)
                        .copied()
                        .unwrap_or(0),
                ));
            }
        }
        SnapshotRegionRevision {
            epoch: self.snapshot_epoch,
            chunks,
        }
    }

    pub(crate) fn take_snapshot_changes(&mut self) -> SnapshotChanges {
        SnapshotChanges {
            all: std::mem::take(&mut self.snapshot_all_dirty),
            chunks: std::mem::take(&mut self.snapshot_dirty_chunks),
            blocks: std::mem::take(&mut self.snapshot_dirty_blocks),
        }
    }

    pub(crate) fn advance_mesh_epoch(&mut self) {
        self.mesh_epoch = self
            .mesh_epoch
            .checked_add(1)
            .expect("background mesh epoch overflow");
        self.mesh_generations.clear();
    }

    /// Reject every background mesh built against a previous texture layout.
    /// Resource-pack reloads change atlas UVs without changing block data, so
    /// content-generation tokens alone cannot identify those stale results.
    pub fn invalidate_mesh_jobs_for_resource_reload(&mut self) {
        self.advance_mesh_epoch();
        self.pending_mesh_queue.clear();
        self.pending_mesh_priority_queue.clear();
        self.pending_mesh_removals.clear();
        self.pending_mesh_inflight.clear();
        self.pending_mesh_dirty.clear();
        self.pending_mesh_priority_inflight.clear();
        self.pending_mesh_priority_dirty.clear();
    }

    pub(crate) fn mesh_job_token(&self, position: (i32, i32)) -> MeshJobToken {
        MeshJobToken {
            epoch: self.mesh_epoch,
            generation: self.mesh_generations.get(&position).copied().unwrap_or(0),
        }
    }

    pub(crate) fn invalidate_chunk_mesh(&mut self, position: (i32, i32)) {
        let generation = self.mesh_generations.entry(position).or_insert(0);
        *generation = generation
            .checked_add(1)
            .expect("background mesh generation overflow");
    }

    pub(crate) fn mark_snapshot_changed(&mut self) {
        self.advance_mesh_epoch();
        self.snapshot_revision = self.snapshot_revision.wrapping_add(1);
        self.snapshot_epoch = self.snapshot_revision;
        self.snapshot_chunk_revisions.clear();
        self.snapshot_dirty_blocks.clear();
        self.snapshot_dirty_chunks.clear();
        self.snapshot_all_dirty = true;
    }

    pub(crate) fn mark_snapshot_chunk_changed(&mut self, cx: i32, cz: i32) {
        self.snapshot_revision = self.snapshot_revision.wrapping_add(1);
        if self.chunks.contains_key(&(cx, cz)) {
            self.snapshot_chunk_revisions
                .insert((cx, cz), self.snapshot_revision);
        } else {
            self.snapshot_chunk_revisions.remove(&(cx, cz));
        }
        self.snapshot_dirty_chunks.insert((cx, cz));
    }

    pub(crate) fn mark_snapshot_chunks_changed(&mut self, chunks: &[(i32, i32)]) {
        if chunks.is_empty() {
            return;
        }
        self.snapshot_revision = self.snapshot_revision.wrapping_add(1);
        let revision = self.snapshot_revision;
        for &(cx, cz) in chunks {
            if self.chunks.contains_key(&(cx, cz)) {
                self.snapshot_chunk_revisions.insert((cx, cz), revision);
            } else {
                self.snapshot_chunk_revisions.remove(&(cx, cz));
            }
            self.snapshot_dirty_chunks.insert((cx, cz));
        }
    }

    pub(crate) fn mark_snapshot_block_changed(&mut self, wx: i32, wy: i32, wz: i32) {
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        self.snapshot_revision = self.snapshot_revision.wrapping_add(1);
        self.snapshot_chunk_revisions
            .insert((cx, cz), self.snapshot_revision);
        self.snapshot_dirty_blocks.insert((wx, wy, wz));
    }
}
