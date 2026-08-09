use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::RhiResult;

use super::super::meshes::*;
use super::super::state::EntityRenderer;
use crate::client::player::Camera;

impl EntityRenderer {
    pub(super) fn sync_particle_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        camera: &Camera,
    ) -> RhiResult<()> {
        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(());
        };
        let particle_hash = self.particle_generation;
        let particle_sort_key = particle_sort_key(camera, &self.particles);
        if self.uploaded_particle_hash == Some(particle_hash) {
            if let Some(draw) = self.particle_draw.as_mut() {
                draw.sort_key = particle_sort_key;
            }
            return Ok(());
        }

        let old_draw = self.particle_draw.take();
        let new_draw = if self.particles.is_empty() {
            None
        } else if let (Some(bind_group), Some(material)) =
            (gpu.block_bind_group, gpu.block_material)
        {
            let (vertices, indices) = crate::render::particle_mesh::build_particle_mesh(
                &self.particles,
                crate::assets::texture::tex_idx,
            );
            let mut draw = crate::render::mesh::upload_entity_mesh(
                runtime,
                gpu.transparent_pipeline,
                Some(bind_group),
                &vertices,
                &indices,
                "game.entity.particles",
            )?;
            if let Some(draw) = draw.as_mut() {
                draw.material = Some(material);
                draw.sort_key = particle_sort_key;
            }
            draw
        } else {
            None
        };
        if let Some(draw) = old_draw {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        self.particle_draw = new_draw;
        self.uploaded_particle_hash = Some(particle_hash);
        Ok(())
    }
}

fn particle_sort_key(camera: &Camera, particles: &[crate::client::particles::Particle]) -> u64 {
    let distance_sq = particles
        .iter()
        .map(|particle| {
            let dx = particle.position.x - camera.position.x;
            let dy = particle.position.y - camera.position.y;
            let dz = particle.position.z - camera.position.z;
            dx * dx + dy * dy + dz * dz
        })
        .fold(0.0, f32::max);
    let distance = distance_sq.sqrt().min(1_000_000.0);
    u64::MAX - (distance * 1024.0) as u64
}
