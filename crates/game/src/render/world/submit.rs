use renderer_api::core::{RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;

use super::WorldRenderer;

impl WorldRenderer {
    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        let Some(gpu) = self.gpu.as_ref() else {
            crate::render::submit_draws(runtime, RenderPassKind::Geometry, &self.draws);
            return;
        };

        if self.visible_camera_key.is_some() {
            for &index in &self.visible_indices {
                submit_chunk_draw(self, runtime, gpu, index);
            }
        } else {
            for index in 0..self.draws.len() {
                submit_chunk_draw(self, runtime, gpu, index);
            }
        }
    }
}

fn submit_chunk_draw<D: RenderDevice>(
    renderer: &WorldRenderer,
    runtime: &mut RendererRuntime<D>,
    gpu: &super::types::WorldGpuState,
    index: usize,
) {
    let Some(draw) = renderer.draws.get(index) else {
        return;
    };
    if renderer
        .visible_draws
        .get(index)
        .is_some_and(|visible| !visible)
    {
        return;
    }
    let transparent_start = renderer
        .transparent_ranges
        .get(index)
        .copied()
        .unwrap_or(0)
        .min(draw.index_count);
    if transparent_start > 0 {
        let mut packet = (*draw).packet(RenderPassKind::Geometry);
        packet.index_count = transparent_start;
        packet.first_index = 0;
        runtime.queue_mut().push(packet);
    }

    if transparent_start < draw.index_count {
        let mut packet = (*draw).packet(RenderPassKind::Transparent);
        packet.pipeline = gpu.transparent_pipeline;
        packet.bind_group = Some(gpu.transparent_bind_group);
        packet.material = Some(gpu.transparent_material);
        packet.first_index = transparent_start;
        packet.index_count = draw.index_count - transparent_start;
        packet.sort_key = renderer
            .transparent_sort_keys
            .get(index)
            .copied()
            .unwrap_or(u64::MAX);
        runtime.queue_mut().push(packet);
    }
}
