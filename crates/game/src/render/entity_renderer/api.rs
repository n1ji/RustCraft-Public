use renderer_api::core::{RendererRuntime, SkyEnvironment};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{BindGroupHandle, PipelineHandle, RhiResult};

use super::state::EntityRenderer;
use crate::render::mesh::upload_mesh;
use crate::render::MeshDraw;

impl EntityRenderer {
    pub fn set_draws(&mut self, draws: Vec<MeshDraw>) {
        self.draws = draws;
    }

    pub fn draws(&self) -> &[MeshDraw] {
        &self.draws
    }

    pub fn pipeline(&self) -> Option<PipelineHandle> {
        self.gpu.as_ref().map(|gpu| gpu.pipeline)
    }

    pub fn transparent_pipeline(&self) -> Option<PipelineHandle> {
        self.gpu.as_ref().map(|gpu| gpu.transparent_pipeline)
    }

    pub fn first_person_pipeline(&self) -> Option<PipelineHandle> {
        self.gpu.as_ref().map(|gpu| gpu.first_person_pipeline)
    }

    pub fn set_particles(
        &mut self,
        particles: &[crate::client::particles::Particle],
        generation: u64,
    ) {
        if self.particle_generation == generation && self.particles.len() == particles.len() {
            return;
        }
        self.particles.clear();
        self.particles.extend_from_slice(particles);
        self.particle_generation = generation;
    }

    pub fn set_environment(&mut self, environment: SkyEnvironment) {
        self.environment = Some(environment);
    }

    pub fn set_block_overlay(
        &mut self,
        boxes: &[crate::client::interaction::SelectionBox],
        dig_progress: f32,
        dig_position: Option<[i32; 3]>,
    ) {
        self.block_selection_boxes.clear();
        self.block_selection_boxes.extend_from_slice(boxes);
        self.dig_progress = dig_progress;
        self.dig_position = dig_position;
    }

    pub fn upload_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        vertices: &[u8],
        indices: &[u32],
        label: &str,
    ) -> RhiResult<()> {
        self.draws
            .push(upload_mesh(runtime, pipeline, vertices, indices, label)?);
        Ok(())
    }

    pub fn upload_entity_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        bind_group: Option<BindGroupHandle>,
        vertices: &[crate::render::entity::mesh::EntityVertex],
        indices: &[u32],
        label: &str,
    ) -> RhiResult<()> {
        let active_bind_group = bind_group.or_else(|| self.gpu.as_ref().map(|gpu| gpu.bind_group));
        let active_pipeline = self.gpu.as_ref().map_or(pipeline, |gpu| gpu.pipeline);
        if let Some(mut draw) = crate::render::mesh::upload_entity_mesh(
            runtime,
            active_pipeline,
            active_bind_group,
            vertices,
            indices,
            label,
        )? {
            if let Some(gpu) = self.gpu.as_ref() {
                draw.material = Some(gpu.material);
            }
            self.draws.push(draw);
        }
        Ok(())
    }
}
