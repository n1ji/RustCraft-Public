mod extras;
mod first_person;
mod overlays;
mod particles;
mod scene;

use renderer_api::core::{RendererRuntime, SkyGradient};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{Extent2D, RhiResult, TextureHandle};

use super::state::{EntityGpuState, EntityRenderer};
use crate::client::player::Camera;
use crate::render::entity::atlas::{
    EntityTextureAtlas, ENTITY_ATLAS_SIZE, PLAYER_SKIN_ATLAS_HEIGHT, PLAYER_SKIN_ATLAS_Y,
};
use crate::render::world::WorldUniforms;

impl EntityRenderer {
    pub fn sync_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        camera: &Camera,
        font: &mut crate::ui::font::FontRenderer,
        hud: &crate::render::hud::HudRenderer,
        settings: &crate::render::settings::RenderSettings,
        underwater: bool,
    ) -> RhiResult<()> {
        self.sync_entity_atlas(runtime)?;
        self.sync_extras_mesh(runtime, font, camera)?;
        self.sync_scene_mesh(runtime, camera)?;
        self.sync_particle_mesh(runtime, camera)?;
        self.sync_block_overlay_mesh(runtime)?;
        self.sync_first_person_mesh(runtime, hud, settings)?;
        let Some(mut gpu) = self.gpu.take() else {
            return Ok(());
        };
        let view = camera.view_matrix();
        let proj = camera.projection_matrix_at(camera.partial_tick);
        let view_proj = proj * view;
        let environment = self
            .environment
            .unwrap_or_else(|| SkyGradient::environment(0, 8, 0));
        let uniforms = WorldUniforms {
            view: crate::render::world::matrix_array(&view),
            proj: crate::render::world::matrix_array(&proj),
            view_proj: crate::render::world::matrix_array(&view_proj),
            prev_view_proj: crate::render::world::matrix_array(&view_proj),
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
                0.0,
            ],
            camera_right: [camera.right.x, camera.right.y, camera.right.z, 0.0],
            camera_up: [camera.up.x, camera.up.y, camera.up.z, 0.0],
        };
        let first_person_proj = camera.first_person_projection_matrix(underwater);
        let partial_tick = camera.partial_tick.clamp(0.0, 1.0);
        let arm_pitch = hud.first_person_prev_arm_pitch
            + crate::util::wrap_degrees(
                hud.first_person_arm_pitch - hud.first_person_prev_arm_pitch,
            ) * partial_tick;
        let arm_yaw = hud.first_person_prev_arm_yaw
            + crate::util::wrap_degrees(hud.first_person_arm_yaw - hud.first_person_prev_arm_yaw)
                * partial_tick;
        let first_person_view =
            crate::render::first_person::camera_effect_transform(camera, arm_pitch, arm_yaw);
        let light_in_view = view
            * nalgebra::Vector4::new(
                environment.light_dir[0],
                environment.light_dir[1],
                environment.light_dir[2],
                0.0,
            );
        let first_person_uniforms = WorldUniforms {
            view: crate::render::world::matrix_array(&first_person_view),
            proj: crate::render::world::matrix_array(&first_person_proj),
            view_proj: crate::render::world::matrix_array(&(first_person_proj * first_person_view)),
            prev_view_proj: crate::render::world::matrix_array(
                &(first_person_proj * first_person_view),
            ),
            light_dir: [
                light_in_view.x,
                light_in_view.y,
                light_in_view.z,
                environment.light_dir[3],
            ],
            fog_color: environment.fog_color,
            fog_params: [
                environment.fog_params[0],
                environment.fog_params[1].min(camera.far),
                0.0,
                environment.fog_params[3].clamp(0.0, 1.0),
            ],
            grass_color: [
                crate::world::material::GRASS_COLOR[0],
                crate::world::material::GRASS_COLOR[1],
                crate::world::material::GRASS_COLOR[2],
                0.0,
            ],
            camera_right: [camera.right.x, camera.right.y, camera.right.z, 0.0],
            camera_up: [camera.up.x, camera.up.y, camera.up.z, 0.0],
        };
        let result =
            runtime
                .device()
                .upload_buffer(gpu.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        if result.is_ok() {
            if let Some(buffer) = gpu.first_person_uniform_buffer {
                if let Err(error) = runtime.device().upload_buffer(
                    buffer,
                    0,
                    bytemuck::bytes_of(&first_person_uniforms),
                ) {
                    self.gpu = Some(gpu);
                    return Err(error);
                }
            }
        }
        if result.is_ok() {
            let desired_texture = self
                .entity_texture
                .filter(|handle| runtime.resources().texture(*handle).is_some())
                .or(gpu.fallback_texture);
            if let Some(desired_texture) = desired_texture {
                if desired_texture != gpu.bound_texture {
                    if let Err(error) =
                        self.rebind_entity_texture(runtime, &mut gpu, desired_texture)
                    {
                        self.gpu = Some(gpu);
                        return Err(error);
                    }
                }
            }
        }
        self.gpu = Some(gpu);
        result
    }

    fn rebind_entity_texture<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        gpu: &mut EntityGpuState,
        texture: TextureHandle,
    ) -> RhiResult<()> {
        // Build every binding that consumes the entity atlas before swapping
        // anything. This prevents one pass from retaining a destroyed atlas
        // binding when skins/resource packs replace the texture at runtime.
        let opaque = Self::create_texture_material(
            runtime,
            gpu.pipeline,
            gpu.uniform_buffer,
            gpu.sampler,
            texture,
        )?;
        let transparent = match Self::create_texture_material(
            runtime,
            gpu.transparent_pipeline,
            gpu.uniform_buffer,
            gpu.sampler,
            texture,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                let _ = runtime.destroy_material(opaque.1);
                let _ = runtime.destroy_bind_group(opaque.0);
                return Err(error);
            }
        };
        let first_person = match gpu.first_person_uniform_buffer {
            Some(uniform_buffer) => match Self::create_texture_material(
                runtime,
                gpu.transparent_pipeline,
                uniform_buffer,
                gpu.sampler,
                texture,
            ) {
                Ok(binding) => Some(binding),
                Err(error) => {
                    let _ = runtime.destroy_material(transparent.1);
                    let _ = runtime.destroy_bind_group(transparent.0);
                    let _ = runtime.destroy_material(opaque.1);
                    let _ = runtime.destroy_bind_group(opaque.0);
                    return Err(error);
                }
            },
            None => None,
        };

        let old_opaque = (gpu.bind_group, gpu.material);
        let old_transparent = (gpu.transparent_bind_group, gpu.transparent_material);
        gpu.bound_texture = texture;
        gpu.bind_group = opaque.0;
        gpu.material = opaque.1;
        gpu.transparent_bind_group = transparent.0;
        gpu.transparent_material = transparent.1;

        for draw in &mut self.draws {
            draw.bind_group = Some(opaque.0);
            draw.material = Some(opaque.1);
        }
        if let Some(draw) = self.scene_draw.as_mut() {
            draw.bind_group = Some(opaque.0);
            draw.material = Some(opaque.1);
        }
        if let Some(draw) = self.local_scene_draw.as_mut() {
            draw.bind_group = Some(opaque.0);
            draw.material = Some(opaque.1);
        }
        if let Some(draw) = self.block_overlay_draw.as_mut() {
            draw.bind_group = Some(transparent.0);
            draw.material = Some(transparent.1);
        }

        let old_first_person = match first_person {
            Some((bind_group, material)) => {
                let old = (gpu.first_person_bind_group, gpu.first_person_material);
                gpu.first_person_bind_group = Some(bind_group);
                gpu.first_person_material = Some(material);
                if let Some(draw) = self.first_person_arm_draw.as_mut() {
                    draw.bind_group = Some(bind_group);
                    draw.material = Some(material);
                }
                Some(old)
            }
            None => None,
        };

        let _ = runtime.destroy_material(old_opaque.1);
        let _ = runtime.destroy_bind_group(old_opaque.0);
        let _ = runtime.destroy_material(old_transparent.1);
        let _ = runtime.destroy_bind_group(old_transparent.0);
        if let Some((bind_group, material)) = old_first_person {
            if let Some(material) = material {
                let _ = runtime.destroy_material(material);
            }
            if let Some(bind_group) = bind_group {
                let _ = runtime.destroy_bind_group(bind_group);
            }
        }
        Ok(())
    }

    fn sync_entity_atlas<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
    ) -> RhiResult<()> {
        let Some(texture) = self.entity_texture else {
            return Ok(());
        };
        let Some(atlas) = self.entity_atlas.as_mut() else {
            return Ok(());
        };

        let mut upload_player_band = false;
        let mut upload_full_atlas = false;
        if self.synced_player_skin_content_hash != Some(self.player_skin_content_hash) {
            if atlas.sync_player_skins(&self.pending_player_skins) {
                upload_player_band = true;
                upload_full_atlas = atlas.take_full_upload_required();
            }
            self.synced_player_skin_content_hash = Some(self.player_skin_content_hash);
        }

        if self.cape_upload_pending {
            if let Some(pixels) = self.cape_pixels.as_deref() {
                atlas.upload_cape(pixels, 64, 32);
                upload_full_atlas = true;
            }
            self.cape_upload_pending = false;
        }
        self.local_skin_upload_pending = false;

        if upload_full_atlas {
            runtime.device().upload_texture_region(
                texture,
                [0, 0],
                Extent2D::new(ENTITY_ATLAS_SIZE, ENTITY_ATLAS_SIZE),
                &atlas.pixels,
                ENTITY_ATLAS_SIZE * 4,
            )?;
        } else if upload_player_band {
            runtime.device().upload_texture_region(
                texture,
                [0, PLAYER_SKIN_ATLAS_Y],
                Extent2D::new(ENTITY_ATLAS_SIZE, PLAYER_SKIN_ATLAS_HEIGHT),
                atlas.player_skin_pixels(),
                ENTITY_ATLAS_SIZE * 4,
            )?;
        }
        Ok(())
    }
}
