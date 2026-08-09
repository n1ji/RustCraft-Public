use nalgebra::Matrix4;

use renderer_api::core::{
    MaterialDesc, RenderPassKind, RendererRuntime, SkyEnvironment, SkyGradient,
};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, Extent2D, MaterialHandle, PipelineHandle, RhiResult, SamplerDesc, SamplerHandle,
    TextureDesc, TextureFormat, TextureHandle, TextureUsage,
};

use crate::client::player::Camera;
use crate::render::custom_sky::CustomSky;

use super::{submit_draws, MeshDraw};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyUniforms {
    pub zenith: [f32; 4],
    pub horizon: [f32; 4],
    pub sun_dir: [f32; 4],
    pub fog_params: [f32; 4],
    pub custom_sky: [f32; 4],
    pub inv_view_proj: [[f32; 4]; 4],
}

struct SkyGpuState {
    pipeline: PipelineHandle,
    uniform_buffer: BufferHandle,
    sampler: SamplerHandle,
    fallback_texture: TextureHandle,
    custom_texture: TextureHandle,
    bind_group: BindGroupHandle,
    material: MaterialHandle,
}

pub struct SkyRenderer {
    environment: SkyEnvironment,
    draws: Vec<MeshDraw>,
    pub custom_sky: Option<CustomSky>,
    pub custom_sky_texture: Option<TextureHandle>,
    pub custom_sky_generation: u64,
    pub viewport: [u32; 2],
    day_time: i64,
    gpu: Option<SkyGpuState>,
}

impl Default for SkyRenderer {
    fn default() -> Self {
        Self {
            environment: SkyGradient::environment(0, 8, 0),
            draws: Vec::new(),
            custom_sky: None,
            custom_sky_texture: None,
            custom_sky_generation: 0,
            viewport: [1280, 720],
            day_time: 0,
            gpu: None,
        }
    }
}

impl SkyRenderer {
    pub fn update_environment(&mut self, day_time: i64, render_distance: u8, dimension: i8) {
        self.environment = SkyGradient::environment(day_time, render_distance, dimension);
        self.day_time = day_time;
    }

    pub fn environment(&self) -> SkyEnvironment {
        self.environment
    }

    pub fn set_custom_sky(&mut self, data: CustomSky, texture: Option<TextureHandle>) {
        self.custom_sky = Some(data);
        self.custom_sky_texture = texture;
        self.custom_sky_generation = self.custom_sky_generation.wrapping_add(1);
    }

    pub fn set_draws(&mut self, draws: Vec<MeshDraw>) {
        self.draws = draws;
    }

    pub fn pipeline(&self) -> Option<PipelineHandle> {
        self.gpu.as_ref().map(|gpu| gpu.pipeline)
    }

    pub fn configure_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
    ) -> RhiResult<()> {
        self.destroy_gpu(runtime);
        let uniform_buffer = runtime.create_buffer(
            BufferDesc::new(
                std::mem::size_of::<SkyUniforms>() as u64,
                BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                "game.sky.uniforms",
            )
            .device_local(),
        )?;
        let sampler = match runtime.create_sampler(SamplerDesc {
            linear_filter: true,
            repeat_u: false,
            repeat_v: false,
            repeat_w: false,
            label: String::from("game.sky.sampler"),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let mut texture_desc = TextureDesc::new(
            Extent2D::new(1, 1),
            TextureFormat::R8G8B8A8Srgb,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        );
        texture_desc.label = String::from("game.sky.fallback-texture");
        let fallback_texture = match runtime.create_texture(texture_desc) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        if let Err(error) = runtime
            .device()
            .upload_texture(fallback_texture, &[0, 0, 0, 0], 4)
        {
            let _ = runtime.destroy_texture(fallback_texture);
            let _ = runtime.destroy_sampler(sampler);
            let _ = runtime.destroy_buffer(uniform_buffer);
            return Err(error);
        }
        let custom_texture = self
            .custom_sky_texture
            .filter(|handle| runtime.resources().texture(*handle).is_some())
            .unwrap_or(fallback_texture);
        let bind_group = match Self::create_bind_group(
            runtime,
            pipeline,
            uniform_buffer,
            fallback_texture,
            custom_texture,
            sampler,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_texture(fallback_texture);
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let material = match runtime.create_material(MaterialDesc {
            pipeline,
            bind_groups: vec![bind_group],
            textures: vec![fallback_texture, custom_texture],
            uniform_buffer: Some(uniform_buffer),
            push_constants: Vec::new(),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_bind_group(bind_group);
                let _ = runtime.destroy_texture(fallback_texture);
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let vertex_bytes = bytemuck::cast_slice(&[[0.0f32, 0.0], [2.0, 0.0], [0.0, 2.0]]);
        let vertex_buffer = match runtime.create_buffer(
            BufferDesc::new(
                vertex_bytes.len() as u64,
                BufferUsage::VERTEX | BufferUsage::TRANSFER_DST,
                "game.sky.fullscreen-triangle",
            )
            .device_local(),
        ) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(bind_group);
                let _ = runtime.destroy_texture(fallback_texture);
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        if let Err(error) = runtime
            .device()
            .upload_buffer(vertex_buffer, 0, vertex_bytes)
        {
            let _ = runtime.destroy_buffer(vertex_buffer);
            let _ = runtime.destroy_material(material);
            let _ = runtime.destroy_bind_group(bind_group);
            let _ = runtime.destroy_texture(fallback_texture);
            let _ = runtime.destroy_sampler(sampler);
            let _ = runtime.destroy_buffer(uniform_buffer);
            return Err(error);
        }
        self.draws = vec![MeshDraw {
            pipeline,
            bind_group: Some(bind_group),
            vertex_buffer,
            index_buffer: None,
            material: Some(material),
            push_constants: None,
            index_count: 3,
            first_index: 0,
            vertex_offset: 0,
            sort_key: 0,
        }];
        self.gpu = Some(SkyGpuState {
            pipeline,
            uniform_buffer,
            sampler,
            fallback_texture,
            custom_texture,
            bind_group,
            material,
        });
        Ok(())
    }

    pub fn sync_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        camera: &Camera,
    ) -> RhiResult<()> {
        let Some(mut gpu) = self.gpu.take() else {
            return Ok(());
        };
        let result = self.sync_gpu_state(runtime, camera, &mut gpu);
        self.gpu = Some(gpu);
        result
    }

    pub fn destroy_gpu<D: RenderDevice>(&mut self, runtime: &mut RendererRuntime<D>) {
        for draw in self.draws.drain(..) {
            let _ = super::mesh::destroy_mesh(runtime, draw);
        }
        let Some(gpu) = self.gpu.take() else {
            return;
        };
        let _ = runtime.destroy_material(gpu.material);
        let _ = runtime.destroy_bind_group(gpu.bind_group);
        let _ = runtime.destroy_buffer(gpu.uniform_buffer);
        let _ = runtime.destroy_sampler(gpu.sampler);
        let _ = runtime.destroy_texture(gpu.fallback_texture);
    }

    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        submit_draws(runtime, RenderPassKind::Sky, &self.draws);
    }

    fn create_bind_group<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: BufferHandle,
        fallback_texture: TextureHandle,
        custom_texture: TextureHandle,
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
                        size: std::mem::size_of::<SkyUniforms>() as u64,
                    },
                ),
                BindGroupEntry::new(
                    1,
                    BindingResource::CombinedImageSampler {
                        texture: fallback_texture,
                        sampler,
                    },
                ),
                BindGroupEntry::new(
                    2,
                    BindingResource::CombinedImageSampler {
                        texture: fallback_texture,
                        sampler,
                    },
                ),
                BindGroupEntry::new(
                    3,
                    BindingResource::CombinedImageSampler {
                        texture: custom_texture,
                        sampler,
                    },
                ),
            ],
        ))
    }

    fn sync_gpu_state<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        camera: &Camera,
        gpu: &mut SkyGpuState,
    ) -> RhiResult<()> {
        // Sky rays are direction-only. Remove camera translation before
        // inversion so moving far from world origin cannot bend the sky or
        // feed the world-space camera position into the ray direction.
        let view_proj = camera.projection_matrix_at(camera.partial_tick) * camera.sky_view_matrix();
        let inverse = view_proj.try_inverse().unwrap_or_else(Matrix4::identity);
        let zenith = SkyGradient::zenith_color(self.day_time as f32);
        let horizon = SkyGradient::horizon_color(self.day_time as f32);
        let custom_alpha = self
            .custom_sky
            .as_ref()
            .map(|custom| {
                custom
                    .layers
                    .iter()
                    .map(|layer| CustomSky::layer_alpha(layer, self.day_time as f32))
                    .fold(0.0, f32::max)
            })
            .unwrap_or(0.0);
        let uniforms = SkyUniforms {
            zenith: [zenith[0], zenith[1], zenith[2], 1.0],
            horizon: [horizon[0], horizon[1], horizon[2], 1.0],
            sun_dir: [
                self.environment.light_dir[0],
                self.environment.light_dir[1],
                self.environment.light_dir[2],
                1.0,
            ],
            fog_params: [
                self.viewport[0].max(1) as f32,
                SkyGradient::moon_phase(self.day_time) as f32,
                self.viewport[1].max(1) as f32,
                self.environment.fog_params[3],
            ],
            custom_sky: [custom_alpha, 0.0, 0.0, 0.0],
            inv_view_proj: matrix_array(&inverse),
        };
        runtime
            .device()
            .upload_buffer(gpu.uniform_buffer, 0, bytemuck::bytes_of(&uniforms))?;

        let desired_custom = self
            .custom_sky_texture
            .filter(|handle| runtime.resources().texture(*handle).is_some())
            .unwrap_or(gpu.fallback_texture);
        if desired_custom != gpu.custom_texture {
            let new_bind_group = Self::create_bind_group(
                runtime,
                gpu.pipeline,
                gpu.uniform_buffer,
                gpu.fallback_texture,
                desired_custom,
                gpu.sampler,
            )?;
            let new_material = match runtime.create_material(MaterialDesc {
                pipeline: gpu.pipeline,
                bind_groups: vec![new_bind_group],
                textures: vec![gpu.fallback_texture, desired_custom],
                uniform_buffer: Some(gpu.uniform_buffer),
                push_constants: Vec::new(),
            }) {
                Ok(handle) => handle,
                Err(error) => {
                    let _ = runtime.destroy_bind_group(new_bind_group);
                    return Err(error);
                }
            };
            let old_material = gpu.material;
            let old_bind_group = gpu.bind_group;
            gpu.custom_texture = desired_custom;
            gpu.bind_group = new_bind_group;
            gpu.material = new_material;
            for draw in &mut self.draws {
                draw.bind_group = Some(new_bind_group);
                draw.material = Some(new_material);
            }
            let _ = runtime.destroy_material(old_material);
            let _ = runtime.destroy_bind_group(old_bind_group);
        }
        Ok(())
    }
}

fn matrix_array(matrix: &Matrix4<f32>) -> [[f32; 4]; 4] {
    let values = matrix.as_slice();
    [
        [values[0], values[1], values[2], values[3]],
        [values[4], values[5], values[6], values[7]],
        [values[8], values[9], values[10], values[11]],
        [values[12], values[13], values[14], values[15]],
    ]
}
