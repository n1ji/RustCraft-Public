use crate::rhi::types::{BufferHandle, Extent2D, PipelineHandle, SamplerHandle, TextureHandle};

use super::super::render_graph::{CompiledRenderGraph, GraphResourceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameDesc {
    pub extent: Extent2D,
    pub enable_post_processing: bool,
}

impl Default for FrameDesc {
    fn default() -> Self {
        Self {
            extent: Extent2D::new(1, 1),
            enable_post_processing: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInputs {
    pub render_width: u32,
    pub render_height: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub enable_post_processing: bool,
    pub targets: FrameTargetHandles,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameTargetHandles {
    pub hdr_color: Option<TextureHandle>,
    pub depth: Option<TextureHandle>,
    pub motion_vectors: Option<TextureHandle>,
    pub post_process: Option<TextureHandle>,
    pub sdr_output: Option<TextureHandle>,
    pub swapchain: Option<TextureHandle>,
}

/// Backend-neutral contract for the built-in HDR-to-SDR boundary. The game
/// layer supplies only RHI handles; the backend owns the shader implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToneMappingConfig {
    pub pipeline: PipelineHandle,
    pub sampler: SamplerHandle,
    pub vertex_buffer: BufferHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameGraphResources {
    pub hdr_color: GraphResourceId,
    pub depth: GraphResourceId,
    pub motion_vectors: GraphResourceId,
    pub post_process: GraphResourceId,
    pub sdr_output: GraphResourceId,
    pub swapchain: GraphResourceId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameGraphPlan {
    pub resources: FrameGraphResources,
    pub graph: CompiledRenderGraph,
    pub physical_textures: Vec<(GraphResourceId, TextureHandle)>,
}

impl FrameGraphPlan {
    pub fn physical_texture(&self, resource: GraphResourceId) -> Option<TextureHandle> {
        self.physical_textures
            .iter()
            .find_map(|(id, handle)| (*id == resource).then_some(*handle))
    }
}
