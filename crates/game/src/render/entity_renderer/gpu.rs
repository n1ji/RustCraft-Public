use renderer_api::core::{MaterialDesc, RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, Extent2D, MaterialHandle, PipelineHandle, RhiResult, SamplerDesc, SamplerHandle,
    TextureDesc, TextureFormat, TextureHandle, TextureUsage,
};

use super::state::{EntityGpuState, EntityRenderer};
use crate::render::submit_draws;
use crate::render::world::WorldUniforms;

impl EntityRenderer {
    pub fn configure_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        transparent_pipeline: PipelineHandle,
        first_person_pipeline: PipelineHandle,
    ) -> RhiResult<()> {
        self.destroy_gpu(runtime);
        let uniform_buffer = runtime.create_buffer(
            BufferDesc::new(
                std::mem::size_of::<WorldUniforms>() as u64,
                BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                "game.entity.uniforms",
            )
            .device_local(),
        )?;
        let sampler = match runtime.create_sampler(SamplerDesc {
            // Entity and skin atlases are also pixel art. Nearest filtering
            // keeps skin texels and transparent borders crisp at any distance.
            linear_filter: false,
            repeat_u: true,
            repeat_v: true,
            repeat_w: false,
            label: String::from("game.entity.sampler"),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let (bound_texture, fallback_texture) = match self
            .entity_texture
            .filter(|handle| runtime.resources().texture(*handle).is_some())
        {
            Some(texture) => (texture, None),
            None => {
                let mut desc = TextureDesc::new(
                    Extent2D::new(1, 1),
                    TextureFormat::R8G8B8A8Srgb,
                    TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
                );
                desc.label = String::from("game.entity.fallback-texture");
                let texture = match runtime.create_texture(desc) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let _ = runtime.destroy_sampler(sampler);
                        let _ = runtime.destroy_buffer(uniform_buffer);
                        return Err(error);
                    }
                };
                if let Err(error) =
                    runtime
                        .device()
                        .upload_texture(texture, &[255, 255, 255, 255], 4)
                {
                    let _ = runtime.destroy_texture(texture);
                    let _ = runtime.destroy_sampler(sampler);
                    let _ = runtime.destroy_buffer(uniform_buffer);
                    return Err(error);
                }
                (texture, Some(texture))
            }
        };
        let bind_group = match Self::create_bind_group(
            runtime,
            pipeline,
            uniform_buffer,
            bound_texture,
            sampler,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let material = match runtime.create_material(MaterialDesc {
            pipeline,
            bind_groups: vec![bind_group],
            textures: vec![bound_texture],
            uniform_buffer: Some(uniform_buffer),
            push_constants: Vec::new(),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_bind_group(bind_group);
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let transparent_bind_group = match Self::create_bind_group(
            runtime,
            transparent_pipeline,
            uniform_buffer,
            bound_texture,
            sampler,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let transparent_material = match runtime.create_material(MaterialDesc {
            pipeline: transparent_pipeline,
            bind_groups: vec![transparent_bind_group],
            textures: vec![bound_texture],
            uniform_buffer: Some(uniform_buffer),
            push_constants: Vec::new(),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_bind_group(transparent_bind_group);
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let (block_bind_group, block_material) = match Self::create_optional_texture_material(
            runtime,
            transparent_pipeline,
            uniform_buffer,
            sampler,
            self.block_texture,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                let _ = runtime.destroy_material(transparent_material);
                let _ = runtime.destroy_bind_group(transparent_bind_group);
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let (item_bind_group, item_material) = match Self::create_optional_texture_material(
            runtime,
            transparent_pipeline,
            uniform_buffer,
            sampler,
            self.item_texture,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                if let Some(material) = block_material {
                    let _ = runtime.destroy_material(material);
                }
                if let Some(bind_group) = block_bind_group {
                    let _ = runtime.destroy_bind_group(bind_group);
                }
                let _ = runtime.destroy_material(transparent_material);
                let _ = runtime.destroy_bind_group(transparent_bind_group);
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        self.gpu = Some(EntityGpuState {
            pipeline,
            transparent_pipeline,
            first_person_pipeline,
            uniform_buffer,
            sampler,
            fallback_texture,
            bound_texture,
            bind_group,
            material,
            transparent_bind_group,
            transparent_material,
            first_person_uniform_buffer: None,
            first_person_bind_group: None,
            first_person_material: None,
            block_bind_group,
            block_material,
            first_person_block_bind_group: None,
            first_person_block_material: None,
            item_bind_group,
            item_material,
            first_person_item_bind_group: None,
            first_person_item_material: None,
        });
        if let Err(error) = self.configure_first_person_gpu(runtime) {
            self.destroy_gpu(runtime);
            return Err(error);
        }
        Ok(())
    }

    fn configure_first_person_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
    ) -> RhiResult<()> {
        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(());
        };
        // First-person geometry needs blending for skin/item cutouts, but it
        // must still write the freshly-cleared hand depth buffer. Reusing the
        // scene transparency pipeline made every cuboid face pass depth and
        // show through the front of the hand.
        let pipeline = gpu.first_person_pipeline;
        let sampler = gpu.sampler;
        let entity_texture = gpu.bound_texture;
        let block_texture = self.block_texture;
        let item_texture = self.item_texture;

        let uniform_buffer = runtime.create_buffer(
            BufferDesc::new(
                std::mem::size_of::<WorldUniforms>() as u64,
                BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                "game.entity.first-person-uniforms",
            )
            .device_local(),
        )?;
        let (bind_group, material) = match Self::create_optional_texture_material(
            runtime,
            pipeline,
            uniform_buffer,
            sampler,
            Some(entity_texture),
        ) {
            Ok((Some(bind_group), Some(material))) => (bind_group, material),
            Ok(_) => {
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(renderer_api::rhi::types::RhiError::InvalidHandle);
            }
            Err(error) => {
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let (block_bind_group, block_material) = match Self::create_optional_texture_material(
            runtime,
            pipeline,
            uniform_buffer,
            sampler,
            block_texture,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let (item_bind_group, item_material) = match Self::create_optional_texture_material(
            runtime,
            pipeline,
            uniform_buffer,
            sampler,
            item_texture,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                if let Some(material) = block_material {
                    let _ = runtime.destroy_material(material);
                }
                if let Some(bind_group) = block_bind_group {
                    let _ = runtime.destroy_bind_group(bind_group);
                }
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };

        let gpu = self.gpu.as_mut().expect("entity GPU state still exists");
        gpu.first_person_uniform_buffer = Some(uniform_buffer);
        gpu.first_person_bind_group = Some(bind_group);
        gpu.first_person_material = Some(material);
        gpu.first_person_block_bind_group = block_bind_group;
        gpu.first_person_block_material = block_material;
        gpu.first_person_item_bind_group = item_bind_group;
        gpu.first_person_item_material = item_material;
        Ok(())
    }

    pub fn destroy_gpu<D: RenderDevice>(&mut self, runtime: &mut RendererRuntime<D>) {
        for draw in self.draws.drain(..) {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.scene_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.local_scene_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.scene_item_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.particle_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.block_overlay_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.extras_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.first_person_arm_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.first_person_block_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        if let Some(draw) = self.first_person_item_draw.take() {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        self.uploaded_scene_hash = None;
        self.uploaded_particle_hash = None;
        self.uploaded_block_overlay_hash = None;
        self.uploaded_extras_hash = None;
        self.uploaded_sign_atlas_hash = None;
        self.uploaded_first_person_hash = None;
        let Some(gpu) = self.gpu.take() else {
            return;
        };
        for material in [
            gpu.first_person_item_material,
            gpu.first_person_block_material,
            gpu.first_person_material,
        ]
        .into_iter()
        .flatten()
        {
            let _ = runtime.destroy_material(material);
        }
        for bind_group in [
            gpu.first_person_item_bind_group,
            gpu.first_person_block_bind_group,
            gpu.first_person_bind_group,
        ]
        .into_iter()
        .flatten()
        {
            let _ = runtime.destroy_bind_group(bind_group);
        }
        if let Some(buffer) = gpu.first_person_uniform_buffer {
            let _ = runtime.destroy_buffer(buffer);
        }
        if let Some(material) = gpu.item_material {
            let _ = runtime.destroy_material(material);
        }
        if let Some(bind_group) = gpu.item_bind_group {
            let _ = runtime.destroy_bind_group(bind_group);
        }
        if let Some(material) = gpu.block_material {
            let _ = runtime.destroy_material(material);
        }
        if let Some(bind_group) = gpu.block_bind_group {
            let _ = runtime.destroy_bind_group(bind_group);
        }
        let _ = runtime.destroy_material(gpu.transparent_material);
        let _ = runtime.destroy_bind_group(gpu.transparent_bind_group);
        let _ = runtime.destroy_material(gpu.material);
        let _ = runtime.destroy_bind_group(gpu.bind_group);
        let _ = runtime.destroy_buffer(gpu.uniform_buffer);
        let _ = runtime.destroy_sampler(gpu.sampler);
        if let Some(texture) = gpu.fallback_texture {
            let _ = runtime.destroy_texture(texture);
        }
    }

    pub(crate) fn create_bind_group<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: BufferHandle,
        texture: TextureHandle,
        sampler: SamplerHandle,
    ) -> RhiResult<BindGroupHandle> {
        runtime.create_bind_group(BindGroupDesc::new(
            pipeline,
            0,
            vec![
                BindGroupEntry::new(
                    0,
                    BindingResource::Buffer {
                        buffer: uniform_buffer,
                        offset: 0,
                        size: std::mem::size_of::<WorldUniforms>() as u64,
                    },
                ),
                BindGroupEntry::new(
                    1,
                    BindingResource::CombinedImageSampler { texture, sampler },
                ),
            ],
        ))
    }

    pub(crate) fn create_texture_material<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: BufferHandle,
        sampler: SamplerHandle,
        texture: TextureHandle,
    ) -> RhiResult<(BindGroupHandle, MaterialHandle)> {
        let bind_group =
            Self::create_bind_group(runtime, pipeline, uniform_buffer, texture, sampler)?;
        let material = match runtime.create_material(MaterialDesc {
            pipeline,
            bind_groups: vec![bind_group],
            textures: vec![texture],
            uniform_buffer: Some(uniform_buffer),
            push_constants: Vec::new(),
        }) {
            Ok(material) => material,
            Err(error) => {
                let _ = runtime.destroy_bind_group(bind_group);
                return Err(error);
            }
        };
        Ok((bind_group, material))
    }

    fn create_optional_texture_material<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: BufferHandle,
        sampler: SamplerHandle,
        texture: Option<TextureHandle>,
    ) -> RhiResult<(Option<BindGroupHandle>, Option<MaterialHandle>)> {
        let Some(texture) = texture.filter(|handle| runtime.resources().texture(*handle).is_some())
        else {
            return Ok((None, None));
        };
        let (bind_group, material) =
            Self::create_texture_material(runtime, pipeline, uniform_buffer, sampler, texture)?;
        Ok((Some(bind_group), Some(material)))
    }
}
