use renderer_api::core::{
    CompiledRenderGraph, FrameDesc, FrameGraphPlan, FrameInputs, GraphError, RenderGraph,
    RenderQueue, Renderer, RendererRuntime, ResourceManager, ToneMappingConfig,
};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{PipelineHandle, RhiResult};
use std::time::Instant;

use super::{
    entity_renderer::EntityRenderer, frame::FrameRenderer, gui_renderer::GuiRenderer,
    hud::HudRenderer, screens::ScreenRenderer, settings::RenderSettings, shaders::ShaderRuntime,
};
use super::{sky::SkyRenderer, world::WorldRenderer, MeshDraw};

/// Backend-neutral game renderer entry point.
///
/// The application supplies a `RenderDevice`; every game subsystem submits
/// only handles and draw intent through the core runtime.
pub struct GameRenderer<D> {
    pub(crate) runtime: RendererRuntime<D>,
    pub(crate) block_atlas: Option<crate::assets::texture::TextureAtlas>,
    pub settings: RenderSettings,
    pub frame: FrameRenderer,
    pub world: WorldRenderer,
    pub sky: SkyRenderer,
    pub entities: EntityRenderer,
    pub gui: GuiRenderer,
    pub hud: HudRenderer,
    pub screens: ScreenRenderer,
    pub shaders: ShaderRuntime,
    /// GUI/HUD geometry is expensive to rebuild.  Keep the last build state
    /// here so the composition root can refresh it at the same cadence as the
    /// old renderer instead of doing all text/item work for every redraw.
    pub(crate) gui_build_at: Option<Instant>,
    pub(crate) gui_build_menu: u32,
    pub(crate) gui_build_underwater: bool,
    pub(crate) gui_build_in_world: bool,
    pub(crate) gui_build_mouse: [f32; 2],
    pub(crate) world_background: bool,
}

impl<D: RenderDevice> GameRenderer<D> {
    pub fn new(device: D) -> Self {
        Self {
            runtime: RendererRuntime::new(device),
            block_atlas: None,
            settings: RenderSettings::default(),
            frame: FrameRenderer::default(),
            world: WorldRenderer::default(),
            sky: SkyRenderer::default(),
            entities: EntityRenderer::default(),
            gui: GuiRenderer::default(),
            hud: HudRenderer::default(),
            screens: ScreenRenderer::default(),
            shaders: ShaderRuntime::default(),
            gui_build_at: None,
            gui_build_menu: u32::MAX,
            gui_build_underwater: false,
            gui_build_in_world: false,
            gui_build_mouse: [f32::NAN, f32::NAN],
            world_background: false,
        }
    }

    pub fn runtime(&self) -> &RendererRuntime<D> {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut RendererRuntime<D> {
        &mut self.runtime
    }

    pub fn prepare_frame(&mut self, inputs: FrameInputs) -> Result<&FrameGraphPlan, GraphError> {
        self.runtime.prepare_frame(inputs)
    }

    pub fn submit(&mut self) {
        self.runtime.queue_mut().clear();
        if self.world_background {
            self.world.submit(&mut self.runtime);
            self.sky.submit(&mut self.runtime);
            self.entities.submit(&mut self.runtime);
        }
        self.gui.submit(&mut self.runtime);
        self.hud.submit(&mut self.runtime);
        self.screens.submit(&mut self.runtime);
    }

    pub fn end_frame(&mut self) -> RhiResult<()> {
        self.runtime.end_frame()
    }

    pub fn set_world_draws(&mut self, draws: Vec<MeshDraw>) {
        self.world.set_draws(draws);
    }

    pub fn set_entity_draws(&mut self, draws: Vec<MeshDraw>) {
        self.entities.set_draws(draws);
    }

    pub fn set_gui_draws(&mut self, draws: Vec<MeshDraw>) {
        self.gui.set_draws(draws);
    }

    pub fn set_hud_draws(&mut self, draws: Vec<MeshDraw>) {
        self.hud.set_draws(draws);
    }

    pub fn set_screen_draws(&mut self, draws: Vec<MeshDraw>) {
        self.screens.set_draws(draws);
    }

    /// Configure the GUI's backend-neutral RHI pipeline. The application
    /// creates the pipeline through its selected backend, then hands only the
    /// opaque RHI handle into the game renderer.
    pub fn configure_gui_pipeline(
        &mut self,
        pipeline: renderer_api::rhi::types::PipelineHandle,
    ) -> RhiResult<()> {
        self.gui.configure_gpu(&mut self.runtime, pipeline)
    }

    pub fn destroy_gui_gpu(&mut self) {
        self.gui.destroy_gpu(&mut self.runtime);
    }

    pub fn configure_world_pipeline(
        &mut self,
        pipeline: renderer_api::rhi::types::PipelineHandle,
        transparent_pipeline: renderer_api::rhi::types::PipelineHandle,
    ) -> RhiResult<()> {
        self.world
            .configure_gpu(&mut self.runtime, pipeline, transparent_pipeline)
    }

    pub fn destroy_world_gpu(&mut self) {
        self.world.destroy_gpu(&mut self.runtime);
    }

    pub fn configure_sky_pipeline(
        &mut self,
        pipeline: renderer_api::rhi::types::PipelineHandle,
    ) -> RhiResult<()> {
        self.sky.configure_gpu(&mut self.runtime, pipeline)
    }

    pub fn destroy_sky_gpu(&mut self) {
        self.sky.destroy_gpu(&mut self.runtime);
    }

    pub fn configure_entity_pipelines(
        &mut self,
        pipeline: renderer_api::rhi::types::PipelineHandle,
        transparent_pipeline: renderer_api::rhi::types::PipelineHandle,
        first_person_pipeline: renderer_api::rhi::types::PipelineHandle,
    ) -> RhiResult<()> {
        self.entities.configure_gpu(
            &mut self.runtime,
            pipeline,
            transparent_pipeline,
            first_person_pipeline,
        )
    }

    pub fn destroy_entity_gpu(&mut self) {
        self.entities.destroy_gpu(&mut self.runtime);
    }

    /// Install the backend-created HDR-to-SDR pass.  The game renderer owns
    /// only the RHI handles; the selected backend owns the shader and native
    /// pipeline implementation.
    pub fn configure_tone_mapping(
        &mut self,
        pipeline: PipelineHandle,
        sampler: renderer_api::rhi::types::SamplerHandle,
        vertex_buffer: renderer_api::rhi::types::BufferHandle,
    ) -> RhiResult<()> {
        self.runtime.set_tone_mapping(Some(ToneMappingConfig {
            pipeline,
            sampler,
            vertex_buffer,
        }))
    }
}

impl<D: RenderDevice> Renderer for GameRenderer<D> {
    fn device(&mut self) -> &mut dyn RenderDevice {
        self.runtime.device()
    }

    fn resources(&self) -> &ResourceManager {
        self.runtime.resources()
    }

    fn resources_mut(&mut self) -> &mut ResourceManager {
        self.runtime.resources_mut()
    }

    fn render_graph(&self) -> &RenderGraph {
        self.runtime.render_graph()
    }

    fn render_graph_mut(&mut self) -> &mut RenderGraph {
        self.runtime.render_graph_mut()
    }

    fn render_queue(&self) -> &RenderQueue {
        self.runtime.queue()
    }

    fn render_queue_mut(&mut self) -> &mut RenderQueue {
        self.runtime.queue_mut()
    }

    fn begin_frame(&mut self, desc: FrameDesc) -> RhiResult<&CompiledRenderGraph> {
        let inputs = FrameInputs {
            render_width: desc.extent.width,
            render_height: desc.extent.height,
            display_width: desc.extent.width,
            display_height: desc.extent.height,
            enable_post_processing: desc.enable_post_processing,
            targets: Default::default(),
        };
        let plan = self
            .runtime
            .prepare_frame(inputs)
            .map_err(|error| renderer_api::rhi::types::RhiError::Message(error.to_string()))?;
        Ok(&plan.graph)
    }

    fn end_frame(&mut self) -> RhiResult<()> {
        GameRenderer::end_frame(self)
    }

    fn frame_index(&self) -> u64 {
        self.runtime.frame_index()
    }
}
