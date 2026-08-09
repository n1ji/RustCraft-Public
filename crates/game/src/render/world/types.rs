use nalgebra::Matrix4;

use renderer_api::rhi::types::{
    BindGroupHandle, BufferHandle, MaterialHandle, PipelineHandle, SamplerHandle, TextureHandle,
};

use crate::world::mesh::ChunkMesh;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WorldUniforms {
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    pub view_proj: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    pub light_dir: [f32; 4],
    pub fog_color: [f32; 4],
    pub fog_params: [f32; 4],
    pub grass_color: [f32; 4],
    /// Camera basis used by GPU-billboarded particles. Keeping it in the
    /// shared world/entity uniform lets particle geometry stay resident while
    /// the view rotates.
    pub camera_right: [f32; 4],
    pub camera_up: [f32; 4],
}

#[derive(Debug)]
pub(super) struct WorldGpuState {
    pub(super) pipeline: PipelineHandle,
    pub(super) transparent_pipeline: PipelineHandle,
    pub(super) uniform_buffer: BufferHandle,
    pub(super) sampler: SamplerHandle,
    pub(super) fallback_texture: Option<TextureHandle>,
    pub(super) bound_texture: TextureHandle,
    pub(super) bind_group: BindGroupHandle,
    pub(super) transparent_bind_group: BindGroupHandle,
    pub(super) material: MaterialHandle,
    pub(super) transparent_material: MaterialHandle,
    pub(super) uploaded_generation: u64,
}

pub(crate) fn matrix_array(matrix: &Matrix4<f32>) -> [[f32; 4]; 4] {
    let values = matrix.as_slice();
    [
        [values[0], values[1], values[2], values[3]],
        [values[4], values[5], values[6], values[7]],
        [values[8], values[9], values[10], values[11]],
        [values[12], values[13], values[14], values[15]],
    ]
}

pub(crate) fn chunk_push_constants(mesh: &ChunkMesh) -> [u8; 16] {
    let mut data = [0u8; 16];
    let values = [mesh.cx as f32 * 16.0, 0.0, mesh.cz as f32 * 16.0, 0.0];
    for (index, value) in values.into_iter().enumerate() {
        data[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
    data
}
