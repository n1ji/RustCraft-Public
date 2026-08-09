mod graph;
mod legacy;
mod resource_api;
mod submit;
mod types;

use std::collections::HashSet;

use crate::rhi::device::RenderDevice;
use crate::rhi::types::{
    BindGroupHandle, Extent2D, TextureDesc, TextureFormat, TextureHandle, TextureUsage,
};

use super::material::MaterialManager;
use super::queue::RenderQueue;
use super::render_graph::{CompiledRenderGraph, RenderGraph};
use super::resource::ResourceManager;

pub use types::{
    FrameDesc, FrameGraphPlan, FrameGraphResources, FrameInputs, FrameTargetHandles,
    ToneMappingConfig,
};

/// Backend-neutral entry point used by renderer clients.
pub trait Renderer {
    fn device(&mut self) -> &mut dyn RenderDevice;
    fn resources(&self) -> &ResourceManager;
    fn resources_mut(&mut self) -> &mut ResourceManager;
    fn render_graph(&self) -> &RenderGraph;
    fn render_graph_mut(&mut self) -> &mut RenderGraph;
    fn render_queue(&self) -> &RenderQueue;
    fn render_queue_mut(&mut self) -> &mut RenderQueue;
    fn begin_frame(
        &mut self,
        desc: FrameDesc,
    ) -> crate::rhi::types::RhiResult<&CompiledRenderGraph>;
    fn end_frame(&mut self) -> crate::rhi::types::RhiResult<()>;
    fn frame_index(&self) -> u64;
}

/// Small compatibility renderer retained for callers that only need the
/// default graph. The game-facing renderer uses `RendererRuntime` below.
pub struct CoreRenderer<D> {
    device: D,
    resources: ResourceManager,
    graph: RenderGraph,
    queue: RenderQueue,
    last_compiled_graph: Option<CompiledRenderGraph>,
    frame_index: u64,
}

/// Backend-neutral runtime state used by the game-facing render systems.
///
/// This type owns RHI handles and graph state only. Native Vulkan objects stay
/// behind the injected `RenderDevice` implementation.
pub struct RendererRuntime<D> {
    pub(crate) device: D,
    pub(crate) resources: ResourceManager,
    pub(crate) graph: RenderGraph,
    pub(crate) queue: RenderQueue,
    pub(crate) materials: MaterialManager,
    pub(crate) last_plan: Option<FrameGraphPlan>,
    frame_graph_key: Option<FrameGraphKey>,
    pub(crate) transient_textures: Vec<TextureHandle>,
    pub(crate) transient_descriptions: Vec<TextureDesc>,
    pub(crate) frame_index: u64,
    pub(crate) tone_mapping: Option<ToneMappingConfig>,
    pub(crate) tone_mapping_bind_group: Option<BindGroupHandle>,
    pub(crate) tone_mapping_bound_source: Option<TextureHandle>,
    pub(crate) tone_mapping_bind_group_config: Option<ToneMappingConfig>,
    validated_graph_key: Option<FrameGraphKey>,
    validated_pipelines: HashSet<(
        super::queue::RenderPassKind,
        crate::rhi::types::PipelineHandle,
    )>,
}

/// Describes the part of a frame that affects render-graph topology or
/// attachment compatibility. Resource handles are intentionally absent: a
/// swapchain rotates handles while retaining the same graph shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrameGraphKey {
    pub(super) render_extent: Extent2D,
    pub(super) display_extent: Extent2D,
    pub(super) enable_post_processing: bool,
    pub(super) targets: [TextureGraphKey; 6],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TextureGraphKey {
    pub(super) imported: bool,
    pub(super) extent: Extent2D,
    pub(super) format: TextureFormat,
    pub(super) usage: TextureUsage,
    pub(super) mip_levels: u32,
    pub(super) array_layers: u32,
}
