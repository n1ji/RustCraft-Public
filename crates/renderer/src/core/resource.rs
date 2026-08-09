use std::collections::HashMap;
use std::fmt;

use crate::rhi::types::{
    BindGroupDesc, BindGroupHandle, BindingResource, BufferDesc, BufferHandle, PipelineDesc,
    PipelineHandle, RenderTargetDesc, RenderTargetHandle, RhiError, RhiResult, SamplerDesc,
    SamplerHandle, ShaderBindingType, ShaderDesc, ShaderHandle, ShaderInterface, ShaderStage,
    TextureDesc, TextureHandle, TextureUsage,
};

#[derive(Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

impl<T> Slot<T> {
    fn occupied(value: T) -> Self {
        Self {
            generation: 1,
            value: Some(value),
        }
    }
}

fn insert_slot<T>(slots: &mut Vec<Slot<T>>, free: &mut Vec<u32>, value: T) -> (u32, u32) {
    if let Some(index) = free.pop() {
        let slot = &mut slots[index as usize];
        slot.generation = slot.generation.wrapping_add(1).max(1);
        slot.value = Some(value);
        (index, slot.generation)
    } else {
        let index = slots.len() as u32;
        slots.push(Slot::occupied(value));
        (index, 1)
    }
}

fn remove_slot<T>(
    slots: &mut [Slot<T>],
    free: &mut Vec<u32>,
    index: u32,
    generation: u32,
) -> Option<T> {
    let slot = slots.get_mut(index as usize)?;
    if slot.generation != generation {
        return None;
    }
    let value = slot.value.take()?;
    free.push(index);
    Some(value)
}

fn get_slot<T>(slots: &[Slot<T>], index: u32, generation: u32) -> Option<&T> {
    let slot = slots.get(index as usize)?;
    if slot.generation == generation {
        slot.value.as_ref()
    } else {
        None
    }
}

fn register_slot<T>(
    slots: &mut Vec<Slot<T>>,
    free: &mut Vec<u32>,
    index: u32,
    generation: u32,
    value: T,
) -> RhiResult<()> {
    if index == u32::MAX || generation == 0 {
        return Err(RhiError::InvalidHandle);
    }

    let index = index as usize;
    if index > slots.len() {
        return Err(RhiError::InvalidHandle);
    }

    if index == slots.len() {
        slots.push(Slot {
            generation,
            value: Some(value),
        });
    } else {
        let slot = &mut slots[index];
        if slot.value.is_some() {
            return Err(RhiError::InvalidHandle);
        }
        slot.generation = generation;
        slot.value = Some(value);
    }
    free.retain(|free_index| *free_index != index as u32);
    Ok(())
}

#[derive(Debug, Default)]
pub struct ResourceManager {
    buffers: Vec<Slot<BufferDesc>>,
    free_buffers: Vec<u32>,
    textures: Vec<Slot<TextureDesc>>,
    free_textures: Vec<u32>,
    external_textures: HashMap<TextureHandle, TextureDesc>,
    shaders: Vec<Slot<ShaderDesc>>,
    free_shaders: Vec<u32>,
    pipelines: Vec<Slot<PipelineDesc>>,
    free_pipelines: Vec<u32>,
    samplers: Vec<Slot<SamplerDesc>>,
    free_samplers: Vec<u32>,
    bind_groups: Vec<Slot<BindGroupDesc>>,
    free_bind_groups: Vec<u32>,
    render_targets: Vec<Slot<RenderTargetDesc>>,
    free_render_targets: Vec<u32>,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_buffer(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle> {
        if desc.size == 0 {
            return Err(RhiError::InvalidDescription("buffer size must be non-zero"));
        }
        let (index, generation) = insert_slot(&mut self.buffers, &mut self.free_buffers, desc);
        Ok(BufferHandle::new(index, generation))
    }

    pub(crate) fn register_buffer(
        &mut self,
        handle: BufferHandle,
        desc: BufferDesc,
    ) -> RhiResult<()> {
        if desc.size == 0 {
            return Err(RhiError::InvalidDescription("buffer size must be non-zero"));
        }
        register_slot(
            &mut self.buffers,
            &mut self.free_buffers,
            handle.index(),
            handle.generation(),
            desc,
        )
    }

    pub fn create_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        if desc.extent.is_empty() || desc.mip_levels == 0 || desc.array_layers == 0 {
            return Err(RhiError::InvalidDescription(
                "texture dimensions must be non-zero",
            ));
        }
        let (index, generation) = insert_slot(&mut self.textures, &mut self.free_textures, desc);
        Ok(TextureHandle::new(index, generation))
    }

    pub fn create_external_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        if desc.extent.is_empty() || desc.mip_levels == 0 || desc.array_layers == 0 {
            return Err(RhiError::InvalidDescription(
                "texture dimensions must be non-zero",
            ));
        }
        let mut index = u32::MAX - 1;
        loop {
            let handle = TextureHandle::new(index, 1);
            if !self.external_textures.contains_key(&handle) && self.texture(handle).is_none() {
                self.import_external_texture(handle, desc)?;
                return Ok(handle);
            }
            if index == 0 {
                return Err(RhiError::Backend(String::from(
                    "external texture handle namespace is exhausted",
                )));
            }
            index -= 1;
        }
    }

    pub fn import_external_texture(
        &mut self,
        handle: TextureHandle,
        desc: TextureDesc,
    ) -> RhiResult<()> {
        if !handle.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        if desc.extent.is_empty() || desc.mip_levels == 0 || desc.array_layers == 0 {
            return Err(RhiError::InvalidDescription(
                "texture dimensions must be non-zero",
            ));
        }
        if self.texture(handle).is_some() {
            return Err(RhiError::InvalidHandle);
        }
        self.external_textures.insert(handle, desc);
        Ok(())
    }

    pub(crate) fn register_texture(
        &mut self,
        handle: TextureHandle,
        desc: TextureDesc,
    ) -> RhiResult<()> {
        if desc.extent.is_empty() || desc.mip_levels == 0 || desc.array_layers == 0 {
            return Err(RhiError::InvalidDescription(
                "texture dimensions must be non-zero",
            ));
        }
        register_slot(
            &mut self.textures,
            &mut self.free_textures,
            handle.index(),
            handle.generation(),
            desc,
        )
    }

    pub fn create_shader(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle> {
        if desc.code.is_empty() {
            return Err(RhiError::InvalidDescription(
                "shader code must be non-empty",
            ));
        }
        let (index, generation) = insert_slot(&mut self.shaders, &mut self.free_shaders, desc);
        Ok(ShaderHandle::new(index, generation))
    }

    pub(crate) fn register_shader(
        &mut self,
        handle: ShaderHandle,
        desc: ShaderDesc,
    ) -> RhiResult<()> {
        if desc.code.is_empty() {
            return Err(RhiError::InvalidDescription(
                "shader code must be non-empty",
            ));
        }
        register_slot(
            &mut self.shaders,
            &mut self.free_shaders,
            handle.index(),
            handle.generation(),
            desc,
        )
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc) -> RhiResult<PipelineHandle> {
        let has_compute = desc.compute_shader.is_some();
        let has_graphics = [
            desc.vertex_shader,
            desc.fragment_shader,
            desc.geometry_shader,
            desc.tessellation_control_shader,
            desc.tessellation_evaluation_shader,
        ]
        .into_iter()
        .any(|handle| handle.is_some());
        if has_compute == has_graphics {
            return Err(RhiError::InvalidDescription(
                "pipeline must be either graphics or compute",
            ));
        }
        self.validate_pipeline_shaders(&desc)?;
        let (index, generation) = insert_slot(&mut self.pipelines, &mut self.free_pipelines, desc);
        Ok(PipelineHandle::new(index, generation))
    }

    pub(crate) fn register_pipeline(
        &mut self,
        handle: PipelineHandle,
        desc: PipelineDesc,
    ) -> RhiResult<()> {
        let has_compute = desc.compute_shader.is_some();
        let has_graphics = [
            desc.vertex_shader,
            desc.fragment_shader,
            desc.geometry_shader,
            desc.tessellation_control_shader,
            desc.tessellation_evaluation_shader,
        ]
        .into_iter()
        .any(|handle| handle.is_some());
        if has_compute == has_graphics {
            return Err(RhiError::InvalidDescription(
                "pipeline must be either graphics or compute",
            ));
        }
        self.validate_pipeline_shaders(&desc)?;
        register_slot(
            &mut self.pipelines,
            &mut self.free_pipelines,
            handle.index(),
            handle.generation(),
            desc,
        )
    }

    pub fn create_sampler(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle> {
        let (index, generation) = insert_slot(&mut self.samplers, &mut self.free_samplers, desc);
        Ok(SamplerHandle::new(index, generation))
    }

    pub(crate) fn register_sampler(
        &mut self,
        handle: SamplerHandle,
        desc: SamplerDesc,
    ) -> RhiResult<()> {
        register_slot(
            &mut self.samplers,
            &mut self.free_samplers,
            handle.index(),
            handle.generation(),
            desc,
        )
    }

    pub fn create_bind_group(&mut self, desc: BindGroupDesc) -> RhiResult<BindGroupHandle> {
        self.validate_bind_group(&desc)?;
        let (index, generation) =
            insert_slot(&mut self.bind_groups, &mut self.free_bind_groups, desc);
        Ok(BindGroupHandle::new(index, generation))
    }

    pub(crate) fn register_bind_group(
        &mut self,
        handle: BindGroupHandle,
        desc: BindGroupDesc,
    ) -> RhiResult<()> {
        self.validate_bind_group(&desc)?;
        register_slot(
            &mut self.bind_groups,
            &mut self.free_bind_groups,
            handle.index(),
            handle.generation(),
            desc,
        )
    }

    pub fn create_render_target(
        &mut self,
        desc: RenderTargetDesc,
    ) -> RhiResult<RenderTargetHandle> {
        if desc.colors.is_empty() && desc.depth.is_none() {
            return Err(RhiError::InvalidDescription(
                "render target must contain an attachment",
            ));
        }
        if desc.colors.iter().any(|handle| !handle.is_valid())
            || desc.depth.is_some_and(|handle| !handle.is_valid())
        {
            return Err(RhiError::InvalidHandle);
        }
        if desc
            .colors
            .iter()
            .any(|handle| self.texture(*handle).is_none())
            || desc
                .depth
                .is_some_and(|handle| self.texture(handle).is_none())
        {
            return Err(RhiError::InvalidHandle);
        }
        let (index, generation) = insert_slot(
            &mut self.render_targets,
            &mut self.free_render_targets,
            desc,
        );
        Ok(RenderTargetHandle::new(index, generation))
    }

    pub fn destroy_buffer(&mut self, handle: BufferHandle) -> RhiResult<BufferDesc> {
        self.validate_buffer_destruction(handle)?;
        remove_slot(
            &mut self.buffers,
            &mut self.free_buffers,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn destroy_texture(&mut self, handle: TextureHandle) -> RhiResult<TextureDesc> {
        self.validate_texture_destruction(handle)?;
        if let Some(desc) = self.external_textures.remove(&handle) {
            return Ok(desc);
        }
        remove_slot(
            &mut self.textures,
            &mut self.free_textures,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn destroy_shader(&mut self, handle: ShaderHandle) -> RhiResult<ShaderDesc> {
        self.validate_shader_destruction(handle)?;
        remove_slot(
            &mut self.shaders,
            &mut self.free_shaders,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn destroy_pipeline(&mut self, handle: PipelineHandle) -> RhiResult<PipelineDesc> {
        self.validate_pipeline_destruction(handle)?;
        remove_slot(
            &mut self.pipelines,
            &mut self.free_pipelines,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn destroy_sampler(&mut self, handle: SamplerHandle) -> RhiResult<SamplerDesc> {
        self.validate_sampler_destruction(handle)?;
        remove_slot(
            &mut self.samplers,
            &mut self.free_samplers,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn destroy_bind_group(&mut self, handle: BindGroupHandle) -> RhiResult<BindGroupDesc> {
        remove_slot(
            &mut self.bind_groups,
            &mut self.free_bind_groups,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn destroy_render_target(
        &mut self,
        handle: RenderTargetHandle,
    ) -> RhiResult<RenderTargetDesc> {
        remove_slot(
            &mut self.render_targets,
            &mut self.free_render_targets,
            handle.index(),
            handle.generation(),
        )
        .ok_or(RhiError::InvalidHandle)
    }

    pub fn buffer(&self, handle: BufferHandle) -> Option<&BufferDesc> {
        get_slot(&self.buffers, handle.index(), handle.generation())
    }

    pub fn texture(&self, handle: TextureHandle) -> Option<&TextureDesc> {
        get_slot(&self.textures, handle.index(), handle.generation())
            .or_else(|| self.external_textures.get(&handle))
    }

    pub fn shader(&self, handle: ShaderHandle) -> Option<&ShaderDesc> {
        get_slot(&self.shaders, handle.index(), handle.generation())
    }

    pub fn pipeline(&self, handle: PipelineHandle) -> Option<&PipelineDesc> {
        get_slot(&self.pipelines, handle.index(), handle.generation())
    }

    pub fn sampler(&self, handle: SamplerHandle) -> Option<&SamplerDesc> {
        get_slot(&self.samplers, handle.index(), handle.generation())
    }

    pub fn bind_group(&self, handle: BindGroupHandle) -> Option<&BindGroupDesc> {
        get_slot(&self.bind_groups, handle.index(), handle.generation())
    }

    pub fn render_target(&self, handle: RenderTargetHandle) -> Option<&RenderTargetDesc> {
        get_slot(&self.render_targets, handle.index(), handle.generation())
    }

    pub fn validate_texture_destruction(&self, handle: TextureHandle) -> RhiResult<()> {
        if self.texture(handle).is_none() {
            return Err(RhiError::InvalidHandle);
        }
        if self
            .render_targets
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|target| target.colors.contains(&handle) || target.depth == Some(handle))
        {
            return Err(RhiError::ResourceInUse(
                "texture is referenced by a render target",
            ));
        }
        if self
            .bind_groups
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|group| {
                group
                    .entries
                    .iter()
                    .any(|entry| binding_resource_uses_texture(entry.resource, handle))
            })
        {
            return Err(RhiError::ResourceInUse(
                "texture is referenced by a bind group",
            ));
        }
        Ok(())
    }

    pub fn validate_buffer_destruction(&self, handle: BufferHandle) -> RhiResult<()> {
        if self.buffer(handle).is_none() {
            return Err(RhiError::InvalidHandle);
        }
        if self
            .bind_groups
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|group| {
                group
                    .entries
                    .iter()
                    .any(|entry| binding_resource_uses_buffer(entry.resource, handle))
            })
        {
            return Err(RhiError::ResourceInUse(
                "buffer is referenced by a bind group",
            ));
        }
        Ok(())
    }

    pub fn validate_sampler_destruction(&self, handle: SamplerHandle) -> RhiResult<()> {
        if self.sampler(handle).is_none() {
            return Err(RhiError::InvalidHandle);
        }
        if self
            .bind_groups
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|group| {
                group
                    .entries
                    .iter()
                    .any(|entry| binding_resource_uses_sampler(entry.resource, handle))
            })
        {
            return Err(RhiError::ResourceInUse(
                "sampler is referenced by a bind group",
            ));
        }
        Ok(())
    }

    pub fn validate_pipeline_destruction(&self, handle: PipelineHandle) -> RhiResult<()> {
        if self.pipeline(handle).is_none() {
            return Err(RhiError::InvalidHandle);
        }
        if self
            .bind_groups
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|group| group.pipeline == handle)
        {
            return Err(RhiError::ResourceInUse(
                "pipeline is referenced by a bind group",
            ));
        }
        Ok(())
    }

    pub fn validate_shader_destruction(&self, handle: ShaderHandle) -> RhiResult<()> {
        if self.shader(handle).is_none() {
            return Err(RhiError::InvalidHandle);
        }
        if self
            .pipelines
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|pipeline| {
                pipeline.vertex_shader == Some(handle)
                    || pipeline.fragment_shader == Some(handle)
                    || pipeline.geometry_shader == Some(handle)
                    || pipeline.tessellation_control_shader == Some(handle)
                    || pipeline.tessellation_evaluation_shader == Some(handle)
                    || pipeline.compute_shader == Some(handle)
            })
        {
            return Err(RhiError::ResourceInUse(
                "shader is referenced by a pipeline",
            ));
        }
        Ok(())
    }

    fn validate_pipeline_shaders(&self, desc: &PipelineDesc) -> RhiResult<()> {
        let has_compute = desc.compute_shader.is_some();
        let has_graphics = [
            desc.vertex_shader,
            desc.fragment_shader,
            desc.geometry_shader,
            desc.tessellation_control_shader,
            desc.tessellation_evaluation_shader,
        ]
        .into_iter()
        .any(|handle| handle.is_some());
        if has_compute == has_graphics {
            return Err(RhiError::InvalidDescription(
                "pipeline must be either graphics or compute",
            ));
        }
        if has_graphics && desc.vertex_shader.is_none() {
            return Err(RhiError::InvalidDescription(
                "graphics pipeline requires a vertex shader",
            ));
        }

        for (handle, expected_stage) in [
            (desc.vertex_shader, ShaderStage::Vertex),
            (desc.fragment_shader, ShaderStage::Fragment),
            (desc.geometry_shader, ShaderStage::Geometry),
            (
                desc.tessellation_control_shader,
                ShaderStage::TessellationControl,
            ),
            (
                desc.tessellation_evaluation_shader,
                ShaderStage::TessellationEvaluation,
            ),
            (desc.compute_shader, ShaderStage::Compute),
        ]
        .into_iter()
        .filter_map(|(handle, stage)| handle.map(|handle| (handle, stage)))
        {
            let Some(shader) = self.shader(handle) else {
                return Err(RhiError::InvalidHandle);
            };
            if shader.stage != expected_stage {
                return Err(RhiError::InvalidDescription(
                    "pipeline shader stage does not match the shader slot",
                ));
            }
        }
        self.validate_vertex_layout(desc)?;
        Ok(())
    }

    fn validate_vertex_layout(&self, desc: &PipelineDesc) -> RhiResult<()> {
        let mut bindings = Vec::with_capacity(desc.vertex_buffers.len());
        let mut locations = Vec::new();
        for layout in &desc.vertex_buffers {
            if layout.stride == 0 || bindings.contains(&layout.binding) {
                return Err(RhiError::InvalidDescription(
                    "vertex buffer bindings must be unique and have a non-zero stride",
                ));
            }
            bindings.push(layout.binding);
            for attribute in &layout.attributes {
                if attribute.binding != layout.binding
                    || locations.contains(&attribute.location)
                    || attribute
                        .offset
                        .checked_add(attribute.format.byte_size())
                        .is_none_or(|end| end > layout.stride)
                {
                    return Err(RhiError::InvalidDescription(
                        "vertex attribute does not fit its buffer layout",
                    ));
                }
                locations.push(attribute.location);
            }
        }
        Ok(())
    }

    fn pipeline_interface(&self, desc: &PipelineDesc) -> RhiResult<ShaderInterface> {
        let mut merged = ShaderInterface::default();
        for handle in [
            desc.vertex_shader,
            desc.fragment_shader,
            desc.geometry_shader,
            desc.tessellation_control_shader,
            desc.tessellation_evaluation_shader,
            desc.compute_shader,
        ]
        .into_iter()
        .flatten()
        {
            let Some(shader) = self.shader(handle) else {
                return Err(RhiError::InvalidHandle);
            };
            merged.push_constant_size = merged
                .push_constant_size
                .max(shader.interface.push_constant_size);
            for binding in &shader.interface.bindings {
                if let Some(existing) = merged.bindings.iter_mut().find(|existing| {
                    existing.set == binding.set && existing.binding == binding.binding
                }) {
                    if existing.ty != binding.ty || existing.array_size != binding.array_size {
                        return Err(RhiError::InvalidDescription(
                            "shader stages disagree about a resource binding",
                        ));
                    }
                    existing.stages |= binding.stages;
                } else {
                    merged.bindings.push(binding.clone());
                }
            }
        }
        Ok(merged)
    }

    fn validate_bind_group(&self, desc: &BindGroupDesc) -> RhiResult<()> {
        let Some(pipeline) = self.pipeline(desc.pipeline) else {
            return Err(RhiError::InvalidHandle);
        };
        let interface = self.pipeline_interface(pipeline)?;
        let mut seen = Vec::with_capacity(desc.entries.len());
        for entry in &desc.entries {
            if !seen.iter().all(|binding| *binding != entry.binding) {
                return Err(RhiError::InvalidDescription(
                    "bind group contains a duplicate binding",
                ));
            }
            seen.push(entry.binding);
            let Some(binding) = interface.binding(desc.set, entry.binding) else {
                return Err(RhiError::InvalidDescription(
                    "bind group binding is not declared by the pipeline",
                ));
            };
            if binding.array_size != 1 {
                return Err(RhiError::InvalidDescription(
                    "bind group arrays are not supported yet",
                ));
            }
            self.validate_binding_resource(binding.ty, entry.resource)?;
        }
        Ok(())
    }

    fn validate_binding_resource(
        &self,
        binding_type: ShaderBindingType,
        resource: BindingResource,
    ) -> RhiResult<()> {
        match (binding_type, resource) {
            (
                ShaderBindingType::UniformBuffer | ShaderBindingType::StorageBuffer,
                BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                },
            ) => {
                let Some(desc) = self.buffer(buffer) else {
                    return Err(RhiError::InvalidHandle);
                };
                let required_usage = if binding_type == ShaderBindingType::UniformBuffer {
                    crate::rhi::types::BufferUsage::UNIFORM
                } else {
                    crate::rhi::types::BufferUsage::STORAGE
                };
                if !desc.usage.contains(required_usage)
                    || offset > desc.size
                    || (size != 0 && size > desc.size - offset)
                {
                    return Err(RhiError::InvalidDescription(
                        "buffer binding range or usage is invalid",
                    ));
                }
                Ok(())
            }
            (ShaderBindingType::SampledTexture, BindingResource::Texture { texture }) => {
                let Some(desc) = self.texture(texture) else {
                    return Err(RhiError::InvalidHandle);
                };
                if !desc.usage.contains(TextureUsage::SAMPLED) {
                    return Err(RhiError::InvalidDescription(
                        "sampled texture binding requires sampled texture usage",
                    ));
                }
                Ok(())
            }
            (ShaderBindingType::StorageTexture, BindingResource::StorageTexture { texture }) => {
                let Some(desc) = self.texture(texture) else {
                    return Err(RhiError::InvalidHandle);
                };
                if !desc.usage.contains(TextureUsage::STORAGE) {
                    return Err(RhiError::InvalidDescription(
                        "storage texture binding requires storage texture usage",
                    ));
                }
                Ok(())
            }
            (ShaderBindingType::Sampler, BindingResource::Sampler { sampler }) => self
                .sampler(sampler)
                .map(|_| ())
                .ok_or(RhiError::InvalidHandle),
            (
                ShaderBindingType::CombinedImageSampler,
                BindingResource::CombinedImageSampler { texture, sampler },
            ) => {
                let Some(desc) = self.texture(texture) else {
                    return Err(RhiError::InvalidHandle);
                };
                if !desc.usage.contains(TextureUsage::SAMPLED) {
                    return Err(RhiError::InvalidDescription(
                        "combined image sampler requires sampled texture usage",
                    ));
                }
                if self.sampler(sampler).is_none() {
                    return Err(RhiError::InvalidHandle);
                }
                Ok(())
            }
            (ShaderBindingType::AccelerationStructure, _) => Err(RhiError::InvalidDescription(
                "acceleration structure bindings are not implemented yet",
            )),
            _ => Err(RhiError::InvalidDescription(
                "bind group resource type does not match shader binding",
            )),
        }
    }

    pub fn live_counts(&self) -> ResourceCounts {
        ResourceCounts {
            buffers: self
                .buffers
                .iter()
                .filter(|slot| slot.value.is_some())
                .count(),
            textures: self
                .textures
                .iter()
                .filter(|slot| slot.value.is_some())
                .count()
                + self.external_textures.len(),
            shaders: self
                .shaders
                .iter()
                .filter(|slot| slot.value.is_some())
                .count(),
            pipelines: self
                .pipelines
                .iter()
                .filter(|slot| slot.value.is_some())
                .count(),
            samplers: self
                .samplers
                .iter()
                .filter(|slot| slot.value.is_some())
                .count(),
            bind_groups: self
                .bind_groups
                .iter()
                .filter(|slot| slot.value.is_some())
                .count(),
            render_targets: self
                .render_targets
                .iter()
                .filter(|slot| slot.value.is_some())
                .count(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceCounts {
    pub buffers: usize,
    pub textures: usize,
    pub shaders: usize,
    pub pipelines: usize,
    pub samplers: usize,
    pub bind_groups: usize,
    pub render_targets: usize,
}

impl fmt::Display for ResourceCounts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "buffers={}, textures={}, shaders={}, pipelines={}, samplers={}, bind_groups={}, render_targets={}",
            self.buffers,
            self.textures,
            self.shaders,
            self.pipelines,
            self.samplers,
            self.bind_groups,
            self.render_targets
        )
    }
}

fn binding_resource_uses_buffer(resource: BindingResource, handle: BufferHandle) -> bool {
    matches!(
        resource,
        BindingResource::Buffer { buffer, .. } if buffer == handle
    )
}

fn binding_resource_uses_texture(resource: BindingResource, handle: TextureHandle) -> bool {
    matches!(
        resource,
        BindingResource::Texture { texture }
            | BindingResource::StorageTexture { texture }
            | BindingResource::CombinedImageSampler { texture, .. }
            if texture == handle
    )
}

fn binding_resource_uses_sampler(resource: BindingResource, handle: SamplerHandle) -> bool {
    matches!(
        resource,
        BindingResource::Sampler { sampler }
            | BindingResource::CombinedImageSampler { sampler, .. }
            if sampler == handle
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhi::types::{BufferUsage, Extent2D, ShaderStage, TextureFormat, TextureUsage};

    #[test]
    fn handles_reject_stale_resources() {
        let mut resources = ResourceManager::new();
        let buffer = resources
            .create_buffer(BufferDesc::new(16, BufferUsage::VERTEX, "test.vertex"))
            .unwrap();
        resources.destroy_buffer(buffer).unwrap();
        assert!(resources.buffer(buffer).is_none());
        let replacement = resources
            .create_buffer(BufferDesc::new(32, BufferUsage::VERTEX, "test.replacement"))
            .unwrap();
        assert_ne!(buffer, replacement);
    }

    #[test]
    fn resources_are_counted() {
        let mut resources = ResourceManager::new();
        resources
            .create_texture(TextureDesc::new(
                Extent2D::new(4, 4),
                TextureFormat::Rgba8Unorm,
                TextureUsage::SAMPLED,
            ))
            .unwrap();
        assert_eq!(resources.live_counts().textures, 1);
    }

    #[test]
    fn external_textures_use_a_disjoint_handle_namespace() {
        let mut resources = ResourceManager::new();
        let regular = resources
            .create_texture(TextureDesc::new(
                Extent2D::new(4, 4),
                TextureFormat::Rgba8Unorm,
                TextureUsage::SAMPLED,
            ))
            .unwrap();
        let external = resources
            .create_external_texture(TextureDesc::new(
                Extent2D::new(4, 4),
                TextureFormat::B8G8R8A8Srgb,
                TextureUsage::COLOR_ATTACHMENT | TextureUsage::PRESENT,
            ))
            .unwrap();
        assert_ne!(regular, external);
        assert!(external.index() > regular.index());
        assert_eq!(resources.live_counts().textures, 2);
        resources.destroy_texture(external).unwrap();
        assert!(resources.texture(external).is_none());
        assert!(resources.texture(regular).is_some());
    }

    #[test]
    fn render_targets_validate_attachment_handles() {
        let mut resources = ResourceManager::new();
        let texture = resources
            .create_texture(TextureDesc::new(
                Extent2D::new(4, 4),
                TextureFormat::Rgba8Unorm,
                TextureUsage::COLOR_ATTACHMENT,
            ))
            .unwrap();
        let target = resources
            .create_render_target(RenderTargetDesc::new(vec![texture], None, "test.target"))
            .unwrap();
        assert_eq!(resources.live_counts().render_targets, 1);
        assert!(resources.render_target(target).is_some());
        resources.destroy_render_target(target).unwrap();
        assert!(resources.render_target(target).is_none());
    }

    #[test]
    fn referenced_textures_cannot_be_destroyed_first() {
        let mut resources = ResourceManager::new();
        let texture = resources
            .create_texture(TextureDesc::new(
                Extent2D::new(4, 4),
                TextureFormat::Rgba8Unorm,
                TextureUsage::COLOR_ATTACHMENT,
            ))
            .unwrap();
        resources
            .create_render_target(RenderTargetDesc::new(vec![texture], None, "target"))
            .unwrap();
        assert_eq!(
            resources.destroy_texture(texture),
            Err(RhiError::ResourceInUse(
                "texture is referenced by a render target"
            ))
        );
    }

    #[test]
    fn render_targets_reject_unregistered_textures() {
        let mut resources = ResourceManager::new();
        assert_eq!(
            resources.create_render_target(RenderTargetDesc::new(
                vec![TextureHandle::new(3, 1)],
                None,
                "invalid.target",
            )),
            Err(RhiError::InvalidHandle)
        );
    }

    #[test]
    fn shaders_cannot_be_destroyed_while_pipeline_uses_them() {
        let mut resources = ResourceManager::new();
        let shader = resources
            .create_shader(ShaderDesc {
                stage: ShaderStage::Vertex,
                code: crate::rhi::types::ShaderCode::spirv(vec![0, 0, 0, 0]),
                entry_point: String::from("main"),
                label: String::from("vertex"),
                interface: crate::rhi::types::ShaderInterface::default(),
            })
            .unwrap();
        resources
            .create_pipeline(PipelineDesc {
                vertex_shader: Some(shader),
                ..PipelineDesc::default()
            })
            .unwrap();
        assert_eq!(
            resources.destroy_shader(shader),
            Err(RhiError::ResourceInUse(
                "shader is referenced by a pipeline"
            ))
        );
    }

    #[test]
    fn mirrored_handles_accept_reused_generations() {
        let mut resources = ResourceManager::new();
        let first = resources
            .create_buffer(BufferDesc::new(16, BufferUsage::VERTEX, "first"))
            .unwrap();
        resources.destroy_buffer(first).unwrap();
        let reused = BufferHandle::new(first.index(), first.generation() + 1);
        resources
            .register_buffer(reused, BufferDesc::new(32, BufferUsage::VERTEX, "reused"))
            .unwrap();
        assert!(resources.buffer(reused).is_some());
        assert!(resources.buffer(first).is_none());
    }
}
