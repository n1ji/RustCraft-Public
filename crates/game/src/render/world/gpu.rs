use std::collections::BTreeMap;

use nalgebra::Matrix4;

use renderer_api::core::{MaterialDesc, RendererRuntime, SkyGradient};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindingResource, BufferDesc, BufferMemory,
    BufferUsage, Extent2D, MaterialHandle, PipelineHandle, RhiResult, SamplerDesc, SamplerHandle,
    TextureDesc, TextureFormat, TextureHandle, TextureUsage,
};

use crate::client::player::Camera;
use crate::render::mesh::{destroy_mesh, upload_mesh_with_bind_group_memory};

use super::state::WorldBatch;
use super::types::WorldGpuState;
use super::{matrix_array, WorldRenderer, WorldUniforms};

const CHUNK_BATCH_SIDE: i32 = 4;

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

        let uniform_buffer = runtime.create_buffer(
            BufferDesc::new(
                std::mem::size_of::<WorldUniforms>() as u64,
                BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                "game.world.uniforms",
            )
            .device_local(),
        )?;
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
        for batch in self.batches.drain(..) {
            destroy_world_batch(runtime, batch);
        }
        self.batch_visible.clear();
        self.batch_transparent_sort_keys.clear();
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
            // Keep the resident GPU batches intact and update only their
            // cheap group-level visibility and transparent sort keys.
            self.update_spatial_caches(camera);
            return Ok(());
        }

        // Rebuild coarse batches when the resident chunk set changes. This is
        // intentionally one upload per batch instead of one buffer/draw per
        // chunk; steady-state frames then submit only a handful of packets.
        self.rebuild_chunk_batches(runtime, gpu)?;
        self.force_full_gpu_upload = false;
        self.pending_gpu_updates.clear();
        self.update_spatial_caches(camera);
        gpu.uploaded_generation = generation;
        Ok(())
    }

    fn rebuild_chunk_batches<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        gpu: &WorldGpuState,
    ) -> RhiResult<()> {
        let old_batches = std::mem::take(&mut self.batches);
        self.batch_visible.clear();
        self.batch_transparent_sort_keys.clear();
        self.visible_camera_key = None;
        self.transparent_sort_camera_key = None;
        destroy_world_batches(runtime, old_batches);

        // Keep the old manually supplied path from retaining a second copy of
        // the chunk geometry once the chunk stream is active.
        for draw in self.draws.drain(..) {
            let _ = destroy_mesh(runtime, draw);
        }
        self.draw_positions.clear();
        self.draw_centers.clear();
        self.transparent_ranges.clear();
        self.transparent_sort_keys.clear();
        self.visible_draws.clear();
        self.visible_indices.clear();

        let mut groups: BTreeMap<(i32, i32), Vec<&crate::world::mesh::ChunkMesh>> = BTreeMap::new();
        for mesh in &self.meshes {
            groups
                .entry((
                    mesh.cx.div_euclid(CHUNK_BATCH_SIDE),
                    mesh.cz.div_euclid(CHUNK_BATCH_SIDE),
                ))
                .or_default()
                .push(mesh);
        }

        let mut new_batches = Vec::with_capacity(groups.len());
        for ((group_x, group_z), meshes) in groups {
            let mut vertices = Vec::new();
            let mut opaque_indices = Vec::new();
            let mut transparent_indices = Vec::new();
            for mesh in meshes {
                let base_vertex = u32::try_from(vertices.len()).map_err(|_| {
                    renderer_api::rhi::types::RhiError::InvalidDescription(
                        "chunk batch has too many vertices",
                    )
                })?;
                let origin_x = (mesh.cx * 16) as f32;
                let origin_z = (mesh.cz * 16) as f32;
                vertices.extend(mesh.vertices.iter().map(|vertex| {
                    let mut world_vertex = *vertex;
                    world_vertex.pos[0] += origin_x;
                    world_vertex.pos[2] += origin_z;
                    world_vertex
                }));

                let transparent_start = usize::try_from(mesh.transparent_start)
                    .unwrap_or(usize::MAX)
                    .min(mesh.indices.len());
                for &index in &mesh.indices[..transparent_start] {
                    opaque_indices.push(base_vertex.checked_add(index).ok_or(
                        renderer_api::rhi::types::RhiError::InvalidDescription(
                            "chunk batch index overflow",
                        ),
                    )?);
                }
                for &index in &mesh.indices[transparent_start..] {
                    transparent_indices.push(base_vertex.checked_add(index).ok_or(
                        renderer_api::rhi::types::RhiError::InvalidDescription(
                            "chunk batch index overflow",
                        ),
                    )?);
                }
            }

            let opaque_index_count = u32::try_from(opaque_indices.len()).map_err(|_| {
                renderer_api::rhi::types::RhiError::InvalidDescription(
                    "chunk batch has too many opaque indices",
                )
            })?;
            let transparent_index_count =
                u32::try_from(transparent_indices.len()).map_err(|_| {
                    renderer_api::rhi::types::RhiError::InvalidDescription(
                        "chunk batch has too many transparent indices",
                    )
                })?;
            if opaque_index_count == 0 && transparent_index_count == 0 {
                continue;
            }
            let transparent_first_index = opaque_index_count;
            opaque_indices.extend(transparent_indices);
            let draw = match upload_mesh_with_bind_group_memory(
                runtime,
                gpu.pipeline,
                Some(gpu.bind_group),
                bytemuck::cast_slice(&vertices),
                &opaque_indices,
                &format!("game.chunk-batch.{group_x}.{group_z}"),
                BufferMemory::DeviceLocal,
            ) {
                Ok(draw) => draw,
                Err(error) => {
                    destroy_world_batches(runtime, new_batches);
                    return Err(error);
                }
            };
            let Some(index_buffer) = draw.index_buffer else {
                let _ = runtime.destroy_buffer(draw.vertex_buffer);
                destroy_world_batches(runtime, new_batches);
                return Err(renderer_api::rhi::types::RhiError::Backend(String::from(
                    "chunk batch upload did not create an index buffer",
                )));
            };
            let min_x = (group_x * CHUNK_BATCH_SIDE * 16) as f32;
            let min_z = (group_z * CHUNK_BATCH_SIDE * 16) as f32;
            new_batches.push(WorldBatch {
                vertex_buffer: draw.vertex_buffer,
                index_buffer,
                opaque_index_count,
                transparent_first_index,
                transparent_index_count,
                aabb_min: [min_x, 0.0, min_z],
                aabb_max: [
                    min_x + (CHUNK_BATCH_SIDE * 16) as f32,
                    256.0,
                    min_z + (CHUNK_BATCH_SIDE * 16) as f32,
                ],
            });
        }
        self.batch_visible.resize(new_batches.len(), true);
        self.batch_transparent_sort_keys
            .resize(new_batches.len(), u64::MAX);
        self.batches = new_batches;
        Ok(())
    }

    fn update_spatial_caches(&mut self, camera: &Camera) {
        if self.batches.is_empty() {
            self.update_visible_draws(camera);
            self.update_transparent_sort_keys(camera);
        } else {
            self.update_visible_batches(camera);
            self.update_batch_transparent_sort_keys(camera);
        }
    }

    fn update_visible_batches(&mut self, camera: &Camera) {
        let key = visibility_camera_key(camera);
        if self.visible_camera_key == Some(key) && self.batch_visible.len() == self.batches.len() {
            return;
        }
        self.batch_visible.resize(self.batches.len(), true);
        let frustum = camera.frustum();
        for (visible, batch) in self.batch_visible.iter_mut().zip(&self.batches) {
            *visible = frustum.test_aabb(batch.aabb_min, batch.aabb_max);
        }
        self.visible_camera_key = Some(key);
        self.visible_indices.clear();
    }

    fn update_batch_transparent_sort_keys(&mut self, camera: &Camera) {
        let key = [
            quantize_camera_position(camera.position.x),
            quantize_camera_position(camera.position.y),
            quantize_camera_position(camera.position.z),
        ];
        if self.transparent_sort_camera_key == Some(key)
            && self.batch_transparent_sort_keys.len() == self.batches.len()
        {
            return;
        }
        self.batch_transparent_sort_keys
            .resize(self.batches.len(), u64::MAX);
        for (sort_key, batch) in self
            .batch_transparent_sort_keys
            .iter_mut()
            .zip(&self.batches)
        {
            let center = [
                (batch.aabb_min[0] + batch.aabb_max[0]) * 0.5,
                (batch.aabb_min[1] + batch.aabb_max[1]) * 0.5,
                (batch.aabb_min[2] + batch.aabb_max[2]) * 0.5,
            ];
            *sort_key = transparent_sort_key(center, camera);
        }
        self.transparent_sort_camera_key = Some(key);
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

fn destroy_world_batch<D: RenderDevice>(runtime: &mut RendererRuntime<D>, batch: WorldBatch) {
    let _ = runtime.destroy_buffer(batch.vertex_buffer);
    let _ = runtime.destroy_buffer(batch.index_buffer);
}

fn destroy_world_batches<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    batches: Vec<WorldBatch>,
) {
    for batch in batches {
        destroy_world_batch(runtime, batch);
    }
}
