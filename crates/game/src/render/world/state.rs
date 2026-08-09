use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupHandle, BufferHandle, PipelineHandle, RhiResult, TextureHandle,
};
use std::collections::HashSet;

use crate::client::interaction::SelectionBox;
use crate::client::particles::Particle;
use crate::render::mesh::upload_mesh;
use crate::render::MeshDraw;
use crate::world::mesh::ChunkMesh;

use super::chunk_push_constants;
use super::types::WorldGpuState;
use renderer_api::core::{RendererRuntime, SkyEnvironment};

/// A spatially coarse batch of chunk geometry.
///
/// The vertex positions are already translated into world space, so both the
/// opaque and transparent draws can share one buffer without changing push
/// constants between chunks. Four-by-four chunk groups keep the number of
/// draw calls low while retaining useful frustum culling at the group level.
#[derive(Clone, Copy)]
pub(super) struct WorldBatch {
    pub(super) vertex_buffer: BufferHandle,
    pub(super) index_buffer: BufferHandle,
    pub(super) opaque_index_count: u32,
    pub(super) transparent_first_index: u32,
    pub(super) transparent_index_count: u32,
    pub(super) aabb_min: [f32; 3],
    pub(super) aabb_max: [f32; 3],
}

#[derive(Default)]
pub struct WorldRenderer {
    pub(super) draws: Vec<MeshDraw>,
    pub(super) batches: Vec<WorldBatch>,
    pub(super) batch_visible: Vec<bool>,
    pub(super) batch_transparent_sort_keys: Vec<u64>,
    pub(super) draw_positions: Vec<Option<(i32, i32)>>,
    pub(super) draw_centers: Vec<Option<[f32; 3]>>,
    pub(super) transparent_ranges: Vec<u32>,
    pub(super) transparent_sort_keys: Vec<u64>,
    /// Chunk visibility generated from the current camera frustum. Resident
    /// buffers are kept intact; off-screen draws are omitted from the queue.
    pub(super) visible_draws: Vec<bool>,
    pub(super) visible_indices: Vec<usize>,
    pub(super) visible_camera_key: Option<[i32; 9]>,
    pub(super) transparent_sort_camera_key: Option<[i32; 3]>,
    pub meshes: Vec<ChunkMesh>,
    pub particles: Vec<Particle>,
    pub particle_generation: u64,
    pub block_texture: Option<TextureHandle>,
    pub normal_texture: Option<TextureHandle>,
    pub specular_texture: Option<TextureHandle>,
    pub max_players: u8,
    pub world_time: i64,
    pub day_time: i64,
    pub dimension: i8,
    pub raining: bool,
    pub rain_level: f32,
    pub thunder_level: f32,
    /// Environment values are supplied by the backend-neutral sky system;
    /// world geometry never needs to know how the sky is implemented.
    pub environment: Option<SkyEnvironment>,
    pub gamemode: u8,
    pub level_type: String,
    pub spawn_position: Option<[i32; 3]>,
    pub world_border_center: [f64; 2],
    pub world_border_diameter: f64,
    pub world_border_warning_blocks: i32,
    pub chunk_count_loaded: usize,
    pub sky_brightness_cached: f32,
    pub block_selection_boxes: Vec<SelectionBox>,
    pub dig_progress: f32,
    pub dig_position: Option<[i32; 3]>,
    pub block_dirty: bool,
    pub underwater: bool,
    pub underwater_yaw: f32,
    pub underwater_pitch: f32,
    pub(super) gpu: Option<WorldGpuState>,
    pub(super) previous_view_proj: Option<nalgebra::Matrix4<f32>>,
    pub(super) mesh_generation: u64,
    pub(super) pending_gpu_updates: HashSet<(i32, i32)>,
    pub(super) force_full_gpu_upload: bool,
}

impl WorldRenderer {
    pub fn replace_meshes(&mut self, meshes: &[ChunkMesh]) {
        self.meshes = meshes
            .iter()
            .filter(|mesh| !mesh.is_empty())
            .cloned()
            .collect();
        self.chunk_count_loaded = self.meshes.len();
        self.mesh_generation = self.mesh_generation.wrapping_add(1);
        self.pending_gpu_updates.clear();
        self.force_full_gpu_upload = true;
        self.invalidate_spatial_caches();
    }

    pub fn update_meshes(&mut self, meshes: &[ChunkMesh]) {
        for mesh in meshes {
            let position = (mesh.cx, mesh.cz);
            if mesh.is_empty() {
                self.meshes
                    .retain(|current| (current.cx, current.cz) != position);
            } else if let Some(current) = self
                .meshes
                .iter_mut()
                .find(|current| (current.cx, current.cz) == position)
            {
                current.clone_from(mesh);
            } else {
                self.meshes.push(mesh.clone());
            }
            self.pending_gpu_updates.insert(position);
        }
        self.chunk_count_loaded = self.meshes.len();
        if !meshes.is_empty() {
            self.mesh_generation = self.mesh_generation.wrapping_add(1);
            self.invalidate_spatial_caches();
        }
    }

    pub fn set_particles(&mut self, particles: &[Particle], generation: u64) {
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

    pub fn set_draws(&mut self, draws: Vec<MeshDraw>) {
        self.draws = draws;
        self.batches.clear();
        self.batch_visible.clear();
        self.batch_transparent_sort_keys.clear();
        self.draw_positions.clear();
        self.draw_positions.resize(self.draws.len(), None);
        self.draw_centers.clear();
        self.draw_centers.resize(self.draws.len(), None);
        self.transparent_ranges.clear();
        self.transparent_sort_keys.clear();
        self.visible_draws.clear();
        self.visible_indices.clear();
        self.visible_camera_key = None;
        self.transparent_sort_camera_key = None;
        self.transparent_ranges.resize(self.draws.len(), 0);
        self.transparent_sort_keys.resize(self.draws.len(), 0);
        self.visible_draws.resize(self.draws.len(), true);
        self.visible_indices = (0..self.draws.len()).collect();
        self.invalidate_spatial_caches();
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
        self.draw_positions.push(None);
        self.draw_centers.push(None);
        self.transparent_ranges.push(0);
        self.transparent_sort_keys.push(0);
        self.visible_draws.push(true);
        self.invalidate_spatial_caches();
        Ok(())
    }

    pub fn upload_chunk_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        bind_group: Option<BindGroupHandle>,
        mesh: &ChunkMesh,
        label: &str,
    ) -> RhiResult<()> {
        self.batches.clear();
        self.batch_visible.clear();
        self.batch_transparent_sort_keys.clear();
        if let Some(draw) =
            crate::render::mesh::upload_chunk_mesh(runtime, pipeline, bind_group, mesh, label)?
        {
            let transparent_start = mesh.transparent_start.min(draw.index_count);
            self.draws.push(MeshDraw {
                push_constants: Some(chunk_push_constants(mesh)),
                ..draw
            });
            self.draw_positions.push(Some((mesh.cx, mesh.cz)));
            self.draw_centers.push(Some(chunk_center(mesh)));
            self.transparent_ranges.push(transparent_start);
            self.transparent_sort_keys.push(u64::MAX);
            self.visible_draws.push(true);
            self.invalidate_spatial_caches();
        }
        Ok(())
    }

    pub(super) fn invalidate_spatial_caches(&mut self) {
        self.visible_camera_key = None;
        self.transparent_sort_camera_key = None;
        self.visible_indices.clear();
    }
}

pub(super) fn chunk_center(mesh: &ChunkMesh) -> [f32; 3] {
    [
        (mesh.aabb_min[0] + mesh.aabb_max[0]) * 0.5,
        (mesh.aabb_min[1] + mesh.aabb_max[1]) * 0.5,
        (mesh.aabb_min[2] + mesh.aabb_max[2]) * 0.5,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(cx: i32, cz: i32, marker: f32) -> ChunkMesh {
        ChunkMesh {
            vertices: vec![crate::world::mesh::Vertex {
                pos: [marker, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                uv: [0.0; 2],
                block_type: 0.0,
                sky_light: 15.0,
                block_light: 0.0,
                ambient_occlusion: 1.0,
            }],
            indices: vec![0],
            cx,
            cz,
            transparent_start: 1,
            aabb_min: [marker, 0.0, 0.0],
            aabb_max: [marker, 0.0, 0.0],
        }
    }

    #[test]
    fn partial_mesh_updates_replace_only_the_named_chunk() {
        let mut renderer = WorldRenderer::default();
        renderer.replace_meshes(&[mesh(0, 0, 1.0), mesh(1, 0, 2.0)]);
        renderer.force_full_gpu_upload = false;
        renderer.update_meshes(&[mesh(0, 0, 3.0)]);

        assert_eq!(renderer.meshes.len(), 2);
        assert_eq!(renderer.meshes[0].vertices[0].pos[0], 3.0);
        assert_eq!(renderer.meshes[1].vertices[0].pos[0], 2.0);
        assert_eq!(renderer.pending_gpu_updates.len(), 1);
        assert!(renderer.pending_gpu_updates.contains(&(0, 0)));
        assert!(!renderer.force_full_gpu_upload);
    }

    #[test]
    fn empty_partial_mesh_removes_only_its_chunk() {
        let mut renderer = WorldRenderer::default();
        renderer.replace_meshes(&[mesh(0, 0, 1.0), mesh(1, 0, 2.0)]);
        renderer.force_full_gpu_upload = false;
        renderer.update_meshes(&[ChunkMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            cx: 0,
            cz: 0,
            transparent_start: 0,
            aabb_min: [f32::MAX; 3],
            aabb_max: [f32::MIN; 3],
        }]);

        assert_eq!(renderer.meshes.len(), 1);
        assert_eq!((renderer.meshes[0].cx, renderer.meshes[0].cz), (1, 0));
        assert!(renderer.pending_gpu_updates.contains(&(0, 0)));
    }
}
