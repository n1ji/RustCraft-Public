use nalgebra::Matrix4;

use renderer_api::core::{MaterialDesc, RendererRuntime, SkyGradient};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindingResource, BufferDesc, BufferUsage,
    Extent2D, MaterialHandle, PipelineHandle, RhiResult, SamplerDesc, SamplerHandle, TextureDesc,
    TextureFormat, TextureHandle, TextureUsage,
};

use crate::client::player::Camera;
use crate::render::mesh::destroy_mesh;

use super::state::chunk_center;
use super::types::WorldGpuState;
use super::{chunk_push_constants, matrix_array, WorldRenderer, WorldUniforms};

impl WorldRenderer {
    /// Create the basic world material and its RHI resources. The concrete
    /// shader pipeline is supplied by the application; this method only knows
    /// the neutral binding contract declared by the built-in shader.
    pub fn configure_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        transparent_pipeline: PipelineHandle,
    ) -> RhiResult<()> {
        self.destroy_gpu(runtime);

        let uniform_buffer = runtime.create_buffer(BufferDesc::new(
            std::mem::size_of::<WorldUniforms>() as u64,
            BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
            "game.world.uniforms",
        ))?;
        let sampler = match runtime.create_sampler(SamplerDesc {
            linear_filter: false,
            repeat_u: true,
            repeat_v: true,
            repeat_w: false,
            label: String::from("game.world.sampler"),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        let (bound_texture, fallback_texture) = match self.world_texture(runtime) {
            Some(texture) => (texture, None),
            None => {
                let mut desc = TextureDesc::new(
                    Extent2D::new(1, 1),
                    TextureFormat::R8G8B8A8Srgb,
                    TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
                );
                desc.label = String::from("game.world.fallback-texture");
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
        let transparent_bind_group = match Self::create_bind_group(
            runtime,
            transparent_pipeline,
            uniform_buffer,
            bound_texture,
            sampler,
        ) {
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
        let material = match runtime.create_material(MaterialDesc {
            pipeline,
            bind_groups: vec![bind_group],
            textures: vec![bound_texture],
            uniform_buffer: Some(uniform_buffer),
            push_constants: Vec::new(),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_bind_group(transparent_bind_group);
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
                let _ = runtime.destroy_material(material);
                let _ = runtime.destroy_bind_group(transparent_bind_group);
                let _ = runtime.destroy_bind_group(bind_group);
                if let Some(texture) = fallback_texture {
                    let _ = runtime.destroy_texture(texture);
                }
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };
        self.gpu = Some(WorldGpuState {
            pipeline,
            transparent_pipeline,
            uniform_buffer,
            sampler,
            fallback_texture,
            bound_texture,
            bind_group,
            transparent_bind_group,
            material,
            transparent_material,
            uploaded_generation: u64::MAX,
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
            let _ = destroy_mesh(runtime, draw);
        }
        self.draw_positions.clear();
        self.draw_centers.clear();
        self.transparent_ranges.clear();
        self.transparent_sort_keys.clear();
        self.visible_draws.clear();
        self.visible_indices.clear();
        self.visible_camera_key = None;
        self.transparent_sort_camera_key = None;
        let Some(gpu) = self.gpu.take() else {
            return;
        };
        let _ = runtime.destroy_material(gpu.material);
        let _ = runtime.destroy_material(gpu.transparent_material);
        let _ = runtime.destroy_bind_group(gpu.bind_group);
        let _ = runtime.destroy_bind_group(gpu.transparent_bind_group);
        let _ = runtime.destroy_buffer(gpu.uniform_buffer);
        let _ = runtime.destroy_sampler(gpu.sampler);
        if let Some(texture) = gpu.fallback_texture {
            let _ = runtime.destroy_texture(texture);
        }
    }

    fn world_texture<D: RenderDevice>(
        &self,
        runtime: &RendererRuntime<D>,
    ) -> Option<TextureHandle> {
        self.block_texture
            .filter(|handle| runtime.resources().texture(*handle).is_some())
    }

    fn create_bind_group<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: renderer_api::rhi::types::BufferHandle,
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

    fn sync_gpu_state<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        camera: &Camera,
        gpu: &mut WorldGpuState,
    ) -> RhiResult<()> {
        let view = camera.view_matrix();
        let proj = camera.projection_matrix_at(camera.partial_tick);
        let view_proj = proj * view;
        let environment = self
            .environment
            .unwrap_or_else(|| SkyGradient::environment(self.day_time, 8, self.dimension));
        let previous = self.previous_view_proj.unwrap_or(view_proj);
        let uniforms = WorldUniforms {
            view: matrix_array(&view),
            proj: matrix_array(&proj),
            view_proj: matrix_array(&view_proj),
            prev_view_proj: matrix_array(&previous),
            light_dir: environment.light_dir,
            fog_color: environment.fog_color,
            fog_params: [
                environment.fog_params[0],
                environment.fog_params[1].min(camera.far),
                camera.position.y,
                environment.fog_params[3].clamp(0.0, 1.0),
            ],
            grass_color: [
                crate::world::material::GRASS_COLOR[0],
                crate::world::material::GRASS_COLOR[1],
                crate::world::material::GRASS_COLOR[2],
                self.dimension as f32,
            ],
            camera_right: [camera.right.x, camera.right.y, camera.right.z, 0.0],
            camera_up: [camera.up.x, camera.up.y, camera.up.z, 0.0],
        };
        runtime
            .device()
            .upload_buffer(gpu.uniform_buffer, 0, bytemuck::bytes_of(&uniforms))?;
        self.previous_view_proj = Some(view_proj);

        if let Some(desired_texture) = self.world_texture(runtime).or(gpu.fallback_texture) {
            if desired_texture != gpu.bound_texture {
                let new_bind_group = Self::create_bind_group(
                    runtime,
                    gpu.pipeline,
                    gpu.uniform_buffer,
                    desired_texture,
                    gpu.sampler,
                )?;
                let new_transparent_bind_group = match Self::create_bind_group(
                    runtime,
                    gpu.transparent_pipeline,
                    gpu.uniform_buffer,
                    desired_texture,
                    gpu.sampler,
                ) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let _ = runtime.destroy_bind_group(new_bind_group);
                        return Err(error);
                    }
                };
                let new_material = match runtime.create_material(MaterialDesc {
                    pipeline: gpu.pipeline,
                    bind_groups: vec![new_bind_group],
                    textures: vec![desired_texture],
                    uniform_buffer: Some(gpu.uniform_buffer),
                    push_constants: Vec::new(),
                }) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let _ = runtime.destroy_bind_group(new_transparent_bind_group);
                        let _ = runtime.destroy_bind_group(new_bind_group);
                        return Err(error);
                    }
                };
                let new_transparent_material = match runtime.create_material(MaterialDesc {
                    pipeline: gpu.transparent_pipeline,
                    bind_groups: vec![new_transparent_bind_group],
                    textures: vec![desired_texture],
                    uniform_buffer: Some(gpu.uniform_buffer),
                    push_constants: Vec::new(),
                }) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let _ = runtime.destroy_material(new_material);
                        let _ = runtime.destroy_bind_group(new_transparent_bind_group);
                        let _ = runtime.destroy_bind_group(new_bind_group);
                        return Err(error);
                    }
                };
                let old_material = gpu.material;
                let old_bind_group = gpu.bind_group;
                let old_transparent_material = gpu.transparent_material;
                let old_transparent_bind_group = gpu.transparent_bind_group;
                gpu.material = new_material;
                gpu.bind_group = new_bind_group;
                gpu.transparent_material = new_transparent_material;
                gpu.transparent_bind_group = new_transparent_bind_group;
                gpu.bound_texture = desired_texture;
                for draw in &mut self.draws {
                    draw.bind_group = Some(new_bind_group);
                    draw.material = Some(new_material);
                }
                let _ = runtime.destroy_material(old_material);
                let _ = runtime.destroy_bind_group(old_bind_group);
                let _ = runtime.destroy_material(old_transparent_material);
                let _ = runtime.destroy_bind_group(old_transparent_bind_group);
            }
        }

        let generation = self.mesh_generation;
        if generation == gpu.uploaded_generation {
            // Transparent chunk ordering depends on the camera, not on mesh
            // generation. Keep the buffers intact and update only sort keys
            // and the cheap resident-chunk visibility mask.
            self.update_visible_draws(camera);
            self.update_transparent_sort_keys(camera);
            return Ok(());
        }

        if self.force_full_gpu_upload || gpu.uploaded_generation == u64::MAX {
            self.rebuild_all_chunk_draws(runtime, gpu)?;
            self.force_full_gpu_upload = false;
            self.pending_gpu_updates.clear();
        } else {
            self.upload_changed_chunk_draws(runtime, gpu)?;
        }
        self.update_visible_draws(camera);
        self.update_transparent_sort_keys(camera);
        gpu.uploaded_generation = generation;
        Ok(())
    }

    fn rebuild_all_chunk_draws<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        gpu: &WorldGpuState,
    ) -> RhiResult<()> {
        let mut new_draws = Vec::new();
        let mut new_positions = Vec::new();
        let mut new_centers = Vec::new();
        let mut new_transparent_ranges = Vec::new();
        for mesh in &self.meshes {
            let Some((draw, transparent_start, center)) = upload_chunk_draw(runtime, gpu, mesh)?
            else {
                continue;
            };
            new_draws.push(draw);
            new_positions.push(Some((mesh.cx, mesh.cz)));
            new_centers.push(Some(center));
            new_transparent_ranges.push(transparent_start);
        }

        let old_draws = std::mem::replace(&mut self.draws, new_draws);
        self.draw_positions = new_positions;
        self.draw_centers = new_centers;
        self.transparent_ranges = new_transparent_ranges;
        self.transparent_sort_keys
            .resize(self.draws.len(), u64::MAX);
        self.visible_draws.resize(self.draws.len(), true);
        for draw in old_draws {
            let _ = destroy_mesh(runtime, draw);
        }
        Ok(())
    }

    fn upload_changed_chunk_draws<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        gpu: &WorldGpuState,
    ) -> RhiResult<()> {
        let updates = self.pending_gpu_updates.iter().copied().collect::<Vec<_>>();
        for position in updates {
            // Allocate the replacement first. If allocation/upload fails, the
            // old draw remains valid and this coordinate stays queued to retry.
            let replacement = self
                .meshes
                .iter()
                .find(|mesh| (mesh.cx, mesh.cz) == position)
                .map(|mesh| upload_chunk_draw(runtime, gpu, mesh))
                .transpose()?
                .flatten();
            let old_index = self
                .draw_positions
                .iter()
                .position(|draw_position| *draw_position == Some(position));

            match (old_index, replacement) {
                (Some(index), Some((draw, transparent_start, center))) => {
                    let old = std::mem::replace(&mut self.draws[index], draw);
                    self.draw_positions[index] = Some(position);
                    self.draw_centers[index] = Some(center);
                    self.transparent_ranges[index] = transparent_start;
                    self.transparent_sort_keys[index] = u64::MAX;
                    let _ = destroy_mesh(runtime, old);
                }
                (Some(index), None) => {
                    let old = self.draws.swap_remove(index);
                    self.draw_positions.swap_remove(index);
                    self.draw_centers.swap_remove(index);
                    self.transparent_ranges.swap_remove(index);
                    self.transparent_sort_keys.swap_remove(index);
                    self.visible_draws.swap_remove(index);
                    let _ = destroy_mesh(runtime, old);
                }
                (None, Some((draw, transparent_start, center))) => {
                    self.draws.push(draw);
                    self.draw_positions.push(Some(position));
                    self.draw_centers.push(Some(center));
                    self.transparent_ranges.push(transparent_start);
                    self.transparent_sort_keys.push(u64::MAX);
                    self.visible_draws.push(true);
                }
                (None, None) => {}
            }
            self.pending_gpu_updates.remove(&position);
        }
        Ok(())
    }

    fn update_transparent_sort_keys(&mut self, camera: &Camera) {
        let key = [
            quantize_camera_position(camera.position.x),
            quantize_camera_position(camera.position.y),
            quantize_camera_position(camera.position.z),
        ];
        if self.transparent_sort_keys.len() != self.draws.len() {
            self.transparent_sort_keys
                .resize(self.draws.len(), u64::MAX);
        }
        if self.transparent_sort_camera_key == Some(key) {
            return;
        }
        for (center, sort_key) in self
            .draw_centers
            .iter()
            .zip(self.transparent_sort_keys.iter_mut())
        {
            *sort_key = center.map_or(u64::MAX, |center| transparent_sort_key(center, camera));
        }
        self.transparent_sort_camera_key = Some(key);
    }

    fn update_visible_draws(&mut self, camera: &Camera) {
        let key = visibility_camera_key(camera);
        if self.visible_camera_key == Some(key) && self.visible_draws.len() == self.draws.len() {
            return;
        }
        self.visible_draws.resize(self.draws.len(), true);
        self.visible_indices.clear();
        let frustum = camera.frustum();
        for index in 0..self.draws.len() {
            let visible = if let Some(Some((cx, cz))) = self.draw_positions.get(index) {
                let min_x = *cx as f32 * 16.0;
                let min_z = *cz as f32 * 16.0;
                // Chunk meshes are spatially bounded to one 16x256x16 section.
                // The conservative vertical bounds avoid packets for chunks
                // outside the current view without risking edge clipping.
                frustum.test_aabb([min_x, 0.0, min_z], [min_x + 16.0, 256.0, min_z + 16.0])
            } else {
                true
            };
            self.visible_draws[index] = visible;
            if visible {
                self.visible_indices.push(index);
            }
        }
        self.visible_camera_key = Some(key);
    }
}

fn quantize_camera_position(value: f32) -> i32 {
    (value * 8.0).round() as i32
}

fn visibility_camera_key(camera: &Camera) -> [i32; 9] {
    [
        quantize_camera_position(camera.position.x),
        quantize_camera_position(camera.position.y),
        quantize_camera_position(camera.position.z),
        (camera.yaw.to_degrees() * 2.0).round() as i32,
        (camera.pitch.to_degrees() * 2.0).round() as i32,
        (camera.fov * 4.0).round() as i32,
        (camera.aspect * 256.0).round() as i32,
        (camera.near * 256.0).round() as i32,
        (camera.far * 4.0).round() as i32,
    ]
}

fn upload_chunk_draw<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    gpu: &WorldGpuState,
    mesh: &crate::world::mesh::ChunkMesh,
) -> RhiResult<Option<(crate::render::MeshDraw, u32, [f32; 3])>> {
    let Some(mut draw) = crate::render::mesh::upload_chunk_mesh(
        runtime,
        gpu.pipeline,
        Some(gpu.bind_group),
        mesh,
        &format!("game.chunk.{}.{}", mesh.cx, mesh.cz),
    )?
    else {
        return Ok(None);
    };
    draw.material = Some(gpu.material);
    draw.push_constants = Some(chunk_push_constants(mesh));
    draw.sort_key = (((mesh.cx as i64 as u64) << 32) ^ mesh.cz as i64 as u64).wrapping_add(1);
    let transparent_start = mesh.transparent_start.min(draw.index_count);
    Ok(Some((draw, transparent_start, chunk_center(mesh))))
}

fn transparent_sort_key(center: [f32; 3], camera: &Camera) -> u64 {
    // Include height: stacked transparent geometry in tall maps otherwise
    // receives identical ordering whenever its chunks share X/Z. Squared
    // distance preserves ordering without a per-frame square root.
    let dx = camera.position.x - center[0];
    let dy = camera.position.y - center[1];
    let dz = camera.position.z - center[2];
    let distance_squared = (dx * dx + dy * dy + dz * dz).min(1_000_000_000_000.0);
    u64::MAX - (distance_squared * 1024.0) as u64
}
