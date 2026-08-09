use renderer_api::core::{RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;

use crate::render::MeshDraw;

use super::WorldRenderer;

impl WorldRenderer {
    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        let Some(gpu) = self.gpu.as_ref() else {
            crate::render::submit_draws(runtime, RenderPassKind::Geometry, &self.draws);
            return;
        };

        if !self.batches.is_empty() {
            for (index, batch) in self.batches.iter().enumerate() {
                if self
                    .batch_visible
                    .get(index)
                    .is_some_and(|visible| !visible)
                {
                    continue;
                }
                let sort_key = self
                    .batch_transparent_sort_keys
                    .get(index)
                    .copied()
                    .unwrap_or(u64::MAX);
                submit_batch(self, runtime, gpu, *batch, sort_key);
            }
            return;
        }

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

fn submit_batch<D: RenderDevice>(
    _renderer: &WorldRenderer,
    runtime: &mut RendererRuntime<D>,
    gpu: &super::types::WorldGpuState,
    batch: super::state::WorldBatch,
    transparent_sort_key: u64,
) {
    // Chunk vertices in a batch are world-space. Explicitly reset the shader's
    // vec3 push constant so a preceding entity/hand draw cannot leak its
    // transform into the shared world geometry.
    let draw = MeshDraw {
        pipeline: gpu.pipeline,
        bind_group: Some(gpu.bind_group),
        vertex_buffer: batch.vertex_buffer,
        index_buffer: Some(batch.index_buffer),
        material: Some(gpu.material),
        push_constants: Some([0; 16]),
        index_count: batch.opaque_index_count,
        first_index: 0,
        vertex_offset: 0,
        sort_key: 0,
    };
    if draw.index_count > 0 {
        runtime
            .queue_mut()
            .push(draw.packet(RenderPassKind::Geometry));
    }

    if batch.transparent_index_count > 0 {
        let transparent_draw = MeshDraw {
            pipeline: gpu.transparent_pipeline,
            bind_group: Some(gpu.transparent_bind_group),
            material: Some(gpu.transparent_material),
            index_count: batch.transparent_index_count,
            first_index: batch.transparent_first_index,
            sort_key: transparent_sort_key,
            ..draw
        };
        runtime
            .queue_mut()
            .push(transparent_draw.packet(RenderPassKind::Transparent));
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
