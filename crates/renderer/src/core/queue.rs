//! Game-facing render packets.
//!
//! Packets contain only RHI handles and render intent. Native command objects
//! stay inside the selected backend.

use crate::rhi::types::{BindGroupHandle, BufferHandle, MaterialHandle, PipelineHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RenderPassKind {
    /// Direction-only background rendered before world depth is initialized.
    Sky,
    Geometry,
    Lighting,
    Transparent,
    /// Camera-space hand and held-item geometry rendered after clearing depth.
    FirstPerson,
    Gui,
    PostProcess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawPacket {
    pub pass: RenderPassKind,
    pub pipeline: PipelineHandle,
    pub bind_group: Option<BindGroupHandle>,
    pub vertex_buffer: BufferHandle,
    pub index_buffer: Option<BufferHandle>,
    pub material: Option<MaterialHandle>,
    pub push_constants: Option<[u8; 16]>,
    pub index_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub sort_key: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RenderQueue {
    packets: Vec<DrawPacket>,
    sorted: bool,
}

impl RenderQueue {
    pub fn clear(&mut self) {
        self.packets.clear();
        self.sorted = false;
    }

    pub fn push(&mut self, packet: DrawPacket) {
        self.packets.push(packet);
        self.sorted = false;
    }

    pub fn packets(&self) -> &[DrawPacket] {
        &self.packets
    }

    pub fn sorted_packets(&mut self) -> &[DrawPacket] {
        if self.sorted {
            return &self.packets;
        }
        self.packets
            // Keep submission order for equal keys.  Some passes contain
            // intentional ordering (for example first-person overlays and
            // fallback UI layers), so an unstable sort can make a correct
            // frame flicker even when every packet has the same sort key.
            .sort_by_key(|packet| (packet.pass, packet.sort_key));
        self.sorted = true;
        &self.packets
    }

    /// Move the sorted frame packets out of the queue without cloning them.
    ///
    /// The command encoder does not borrow the queue, so keeping a second
    /// `Vec<DrawPacket>` alive here only doubled the per-frame packet traffic.
    pub(crate) fn take_sorted_packets(&mut self) -> Vec<DrawPacket> {
        self.sorted_packets();
        std::mem::take(&mut self.packets)
    }

    pub(crate) fn restore_packets(&mut self, packets: Vec<DrawPacket>) {
        self.packets = packets;
        self.sorted = true;
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_sorts_by_pass_then_sort_key() {
        let mut queue = RenderQueue::default();
        for (pass, sort_key) in [
            (RenderPassKind::Gui, 0),
            (RenderPassKind::Geometry, 20),
            (RenderPassKind::Geometry, 10),
        ] {
            queue.push(DrawPacket {
                pass,
                pipeline: PipelineHandle::new(1, 1),
                bind_group: None,
                vertex_buffer: BufferHandle::new(1, 1),
                index_buffer: None,
                material: None,
                push_constants: None,
                index_count: 3,
                first_index: 0,
                vertex_offset: 0,
                sort_key,
            });
        }
        let packets = queue.sorted_packets();
        assert_eq!(packets[0].sort_key, 10);
        assert_eq!(packets[1].sort_key, 20);
        assert_eq!(packets[2].pass, RenderPassKind::Gui);
    }
}
