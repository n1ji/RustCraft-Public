use crate::rhi::device::RenderDevice;
use crate::rhi::types::{
    BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, DeviceCapabilities, MaterialHandle,
    PipelineDesc, PipelineHandle, RenderTargetDesc, RenderTargetHandle, RhiError, RhiResult,
    SamplerDesc, SamplerHandle, ShaderDesc, ShaderHandle, TextureDesc, TextureHandle,
};

use super::{RendererRuntime, ToneMappingConfig};
use crate::core::material::MaterialDesc;

impl<D: RenderDevice> RendererRuntime<D> {
    pub fn new(device: D) -> Self {
        Self {
            device,
            resources: super::super::resource::ResourceManager::new(),
            graph: super::super::render_graph::RenderGraph::new(),
            queue: super::super::queue::RenderQueue::default(),
            materials: super::super::material::MaterialManager::new(),
            last_plan: None,
            frame_graph_key: None,
            transient_textures: Vec::new(),
            transient_descriptions: Vec::new(),
            frame_index: 0,
            tone_mapping: None,
            tone_mapping_bind_group: None,
            tone_mapping_bound_source: None,
            tone_mapping_bind_group_config: None,
            validated_graph_key: None,
            validated_pipelines: std::collections::HashSet::new(),
        }
    }

    pub fn into_device(self) -> D {
        self.device
    }

    pub fn backend_name(&self) -> &'static str {
        "rhi"
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        self.device.capabilities()
    }

    pub fn device(&mut self) -> &mut D {
        &mut self.device
    }

    pub fn backend(&self) -> &D {
        &self.device
    }

    pub fn backend_mut(&mut self) -> &mut D {
        &mut self.device
    }

    pub fn resources(&self) -> &super::super::resource::ResourceManager {
        &self.resources
    }

    pub fn resources_mut(&mut self) -> &mut super::super::resource::ResourceManager {
        &mut self.resources
    }

    pub fn render_graph(&self) -> &super::super::render_graph::RenderGraph {
        &self.graph
    }

    pub fn render_graph_mut(&mut self) -> &mut super::super::render_graph::RenderGraph {
        // Callers using the low-level graph API may change pass topology. Do
        // not let the frame cache reuse a compiled graph after that mutation.
        self.frame_graph_key = None;
        self.last_plan = None;
        &mut self.graph
    }

    pub fn queue(&self) -> &super::super::queue::RenderQueue {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut super::super::queue::RenderQueue {
        &mut self.queue
    }

    pub fn set_tone_mapping(&mut self, config: Option<ToneMappingConfig>) -> RhiResult<()> {
        if self.tone_mapping != config {
            self.discard_tone_mapping_bind_group()?;
            self.tone_mapping = config;
        }
        Ok(())
    }

    pub fn materials(&self) -> &super::super::material::MaterialManager {
        &self.materials
    }

    pub fn materials_mut(&mut self) -> &mut super::super::material::MaterialManager {
        &mut self.materials
    }

    pub fn create_material(&mut self, desc: MaterialDesc) -> RhiResult<MaterialHandle> {
        self.materials.create_for_resources(&self.resources, desc)
    }

    pub fn destroy_material(&mut self, handle: MaterialHandle) -> RhiResult<MaterialDesc> {
        self.materials.destroy(handle)
    }

    pub fn create_buffer(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle> {
        let handle = self.device.create_buffer(desc.clone())?;
        if let Err(error) = self.resources.register_buffer(handle, desc) {
            let _ = self.device.destroy_buffer(handle);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn create_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        let handle = self.device.create_texture(desc.clone())?;
        if let Err(error) = self.resources.register_texture(handle, desc) {
            let _ = self.device.destroy_texture(handle);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn register_external_texture(
        &mut self,
        handle: TextureHandle,
        desc: TextureDesc,
    ) -> RhiResult<()> {
        self.resources.import_external_texture(handle, desc)
    }

    pub fn create_shader(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle> {
        let handle = self.device.create_shader(desc.clone())?;
        if let Err(error) = self.resources.register_shader(handle, desc) {
            let _ = self.device.destroy_shader(handle);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc) -> RhiResult<PipelineHandle> {
        let handle = self.device.create_pipeline(desc.clone())?;
        if let Err(error) = self.resources.register_pipeline(handle, desc) {
            let _ = self.device.destroy_pipeline(handle);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn create_sampler(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle> {
        let handle = self.device.create_sampler(desc.clone())?;
        if let Err(error) = self.resources.register_sampler(handle, desc) {
            let _ = self.device.destroy_sampler(handle);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn create_bind_group(&mut self, desc: BindGroupDesc) -> RhiResult<BindGroupHandle> {
        let handle = self.device.create_bind_group(desc.clone())?;
        if let Err(error) = self.resources.register_bind_group(handle, desc) {
            let _ = self.device.destroy_bind_group(handle);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn destroy_buffer(&mut self, handle: BufferHandle) -> RhiResult<BufferDesc> {
        if self.materials.references_buffer(handle) {
            return Err(RhiError::ResourceInUse(
                "buffer is referenced by a material",
            ));
        }
        self.resources.validate_buffer_destruction(handle)?;
        self.device.destroy_buffer(handle)?;
        self.resources.destroy_buffer(handle)
    }

    pub fn destroy_texture(&mut self, handle: TextureHandle) -> RhiResult<TextureDesc> {
        if self.materials.references_texture(handle) {
            return Err(RhiError::ResourceInUse(
                "texture is referenced by a material",
            ));
        }
        self.resources.validate_texture_destruction(handle)?;
        self.device.destroy_texture(handle)?;
        self.resources.destroy_texture(handle)
    }

    pub fn destroy_sampler(&mut self, handle: SamplerHandle) -> RhiResult<SamplerDesc> {
        self.resources.validate_sampler_destruction(handle)?;
        self.device.destroy_sampler(handle)?;
        self.resources.destroy_sampler(handle)
    }

    pub fn destroy_bind_group(&mut self, handle: BindGroupHandle) -> RhiResult<BindGroupDesc> {
        if self.materials.references_bind_group(handle) {
            return Err(RhiError::ResourceInUse(
                "bind group is referenced by a material",
            ));
        }
        self.device.destroy_bind_group(handle)?;
        self.resources.destroy_bind_group(handle)
    }

    pub fn destroy_shader(&mut self, handle: ShaderHandle) -> RhiResult<ShaderDesc> {
        self.resources.validate_shader_destruction(handle)?;
        self.device.destroy_shader(handle)?;
        self.resources.destroy_shader(handle)
    }

    pub fn destroy_pipeline(&mut self, handle: PipelineHandle) -> RhiResult<PipelineDesc> {
        if self.materials.references_pipeline(handle) {
            return Err(RhiError::ResourceInUse(
                "pipeline is referenced by a material",
            ));
        }
        self.device.destroy_pipeline(handle)?;
        self.resources.destroy_pipeline(handle)
    }

    pub fn create_render_target(
        &mut self,
        desc: RenderTargetDesc,
    ) -> RhiResult<RenderTargetHandle> {
        self.resources.create_render_target(desc)
    }

    pub fn destroy_render_target(
        &mut self,
        handle: RenderTargetHandle,
    ) -> RhiResult<RenderTargetDesc> {
        self.resources.destroy_render_target(handle)
    }

    pub fn last_plan(&self) -> Option<&super::types::FrameGraphPlan> {
        self.last_plan.as_ref()
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }

    pub(crate) fn discard_tone_mapping_bind_group(&mut self) -> RhiResult<()> {
        if let Some(bind_group) = self.tone_mapping_bind_group.take() {
            self.device.destroy_bind_group(bind_group)?;
            self.resources.destroy_bind_group(bind_group)?;
        }
        self.tone_mapping_bound_source = None;
        self.tone_mapping_bind_group_config = None;
        Ok(())
    }
}
