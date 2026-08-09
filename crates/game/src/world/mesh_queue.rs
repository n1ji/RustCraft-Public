use super::chunk::CHUNK_SIZE;
use super::helpers::chunk_and_border_neighbors;
use super::state::World;

impl World {
    pub fn enqueue_all_chunks_for_mesh(&mut self) {
        self.advance_mesh_epoch();
        self.pending_mesh_queue.clear();
        self.pending_mesh_priority_queue.clear();
        self.pending_mesh_removals.clear();
        self.pending_mesh_inflight.clear();
        self.pending_mesh_dirty.clear();
        self.pending_mesh_priority_inflight.clear();
        self.pending_mesh_priority_dirty.clear();
        let chunks: Vec<(i32, i32)> = self.chunks.keys().copied().collect();
        for (cx, cz) in chunks {
            self.enqueue_chunk_mesh(cx, cz);
        }
    }

    pub fn enqueue_chunk_mesh(&mut self, cx: i32, cz: i32) {
        let position = (cx, cz);
        if !self.chunks.contains_key(&position) {
            return;
        }
        self.invalidate_chunk_mesh(position);
        if self.pending_mesh_inflight.contains_key(&position) {
            self.pending_mesh_dirty.insert(position);
        } else if !self.pending_mesh_queue.contains(&position)
            && !self.pending_mesh_priority_queue.contains(&position)
        {
            self.pending_mesh_queue.push_back((cx, cz));
        }
    }

    pub(crate) fn enqueue_chunk_mesh_priority(&mut self, cx: i32, cz: i32) {
        let position = (cx, cz);
        if !self.chunks.contains_key(&position) {
            return;
        }
        self.invalidate_chunk_mesh(position);
        if self.pending_mesh_inflight.contains_key(&position) {
            self.pending_mesh_dirty.insert(position);
            self.pending_mesh_priority_dirty.insert(position);
        } else {
            self.pending_mesh_queue.retain(|queued| *queued != position);
            if !self.pending_mesh_priority_queue.contains(&position) {
                self.pending_mesh_priority_queue.push_back(position);
            }
        }
    }

    pub fn enqueue_chunk_mesh_with_neighbors(&mut self, cx: i32, cz: i32) {
        self.enqueue_chunk_mesh(cx, cz);
        self.enqueue_chunk_mesh(cx - 1, cz);
        self.enqueue_chunk_mesh(cx + 1, cz);
        self.enqueue_chunk_mesh(cx, cz - 1);
        self.enqueue_chunk_mesh(cx, cz + 1);
    }

    pub fn enqueue_chunk_mesh_at_block(&mut self, wx: i32, wz: i32) {
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        for (ncx, ncz) in chunk_and_border_neighbors(wx, wz) {
            self.enqueue_chunk_mesh(ncx, ncz);
        }
        self.enqueue_chunk_mesh_priority(cx, cz);
    }

    pub(crate) fn enqueue_chunk_mesh_at_block_normal(&mut self, wx: i32, wz: i32) {
        let cx = wx.div_euclid(CHUNK_SIZE as i32);
        let cz = wz.div_euclid(CHUNK_SIZE as i32);
        self.enqueue_chunk_mesh(cx, cz);
        for (ncx, ncz) in chunk_and_border_neighbors(wx, wz) {
            self.enqueue_chunk_mesh(ncx, ncz);
        }
    }

    pub fn enqueue_chunk_mesh_removal(&mut self, cx: i32, cz: i32) {
        let position = (cx, cz);
        self.invalidate_chunk_mesh(position);
        self.pending_mesh_queue.retain(|queued| *queued != position);
        self.pending_mesh_priority_queue
            .retain(|queued| *queued != position);
        self.pending_mesh_inflight.remove(&position);
        self.pending_mesh_dirty.remove(&position);
        self.pending_mesh_priority_inflight.remove(&position);
        self.pending_mesh_priority_dirty.remove(&position);
        if !self.pending_mesh_removals.contains(&position) {
            self.pending_mesh_removals.push_back(position);
        }
    }

    pub fn has_pending_mesh_work(&self) -> bool {
        !self.pending_mesh_removals.is_empty()
            || !self.pending_mesh_priority_queue.is_empty()
            || !self.pending_mesh_queue.is_empty()
    }
}
