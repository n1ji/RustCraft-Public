//! Composition-root operations shared by the game renderer systems.
//!
//! These methods preserve the game-facing renderer API while routing resource
//! lifetime through `RendererRuntime`. They intentionally contain no Vulkan
//! types or backend-specific ownership.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use renderer_api::core::{FrameInputs, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    Extent2D, ShaderCode, ShaderDesc, SurfaceConfig, TextureDesc, TextureFormat, TextureHandle,
    TextureUsage,
};

use crate::assets::resolver::AssetResolver;
use crate::assets::texture::{TextureAnimationContext, TextureAtlas};
use crate::client::player::Camera;
use crate::render::custom_sky::CustomSky;
use crate::render::entity::atlas::{EntityTextureAtlas, ENTITY_ATLAS_SIZE};
use crate::render::item_icons::{ITEM_ATLAS_H, ITEM_ATLAS_W};
use crate::world::mesh::ChunkMesh;

use super::gui_renderer::{CreativeItemEntry, GuiRenderer, GuiTextureKind};
use super::renderer::GameRenderer;

fn decode_gui_image(
    resolver: &mut AssetResolver,
    resource_path: &str,
) -> Result<image::RgbaImage, String> {
    let bytes = resolver
        .read_bytes(resource_path)
        .ok_or_else(|| format!("missing GUI texture '{resource_path}'"))?;
    crate::assets::texture::decode_image_with_limits(&bytes, 8_192, 256 * 1024 * 1024)
        .map_err(|error| format!("failed to decode GUI texture '{resource_path}': {error}"))
        .map(|image| image.to_rgba8())
}

fn build_gui_grid_atlas(
    resolver: &mut AssetResolver,
    resource_paths: &[&str],
    columns: u32,
    rows: u32,
) -> Result<image::RgbaImage, String> {
    if resource_paths.len() as u32 > columns.saturating_mul(rows) {
        return Err(String::from("GUI grid atlas has more images than cells"));
    }
    let mut images = Vec::with_capacity(resource_paths.len());
    for path in resource_paths {
        images.push(decode_gui_image(resolver, path)?);
    }
    let scale = images
        .iter()
        .map(|image| image.width().max(image.height()).div_ceil(256).max(1))
        .max()
        .unwrap_or(1);
    let cell = 256 * scale;
    let mut atlas = image::RgbaImage::new(cell * columns.max(1), cell * rows.max(1));
    for (index, image) in images.into_iter().enumerate() {
        let image = if image.width() == cell && image.height() == cell {
            image
        } else {
            image::imageops::resize(&image, cell, cell, image::imageops::FilterType::Nearest)
        };
        let x = index as u32 % columns.max(1) * cell;
        let y = index as u32 / columns.max(1) * cell;
        image::imageops::replace(&mut atlas, &image, x as i64, y as i64);
    }
    Ok(atlas)
}

fn upload_gui_image<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    image: image::RgbaImage,
    label: &str,
) -> Result<TextureHandle, String> {
    let (width, height) = image.dimensions();
    GuiRenderer::upload_texture(runtime, width, height, image.as_raw(), label)
        .map_err(|error| error.to_string())
}

fn replace_uploaded_texture<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    current: &mut Option<TextureHandle>,
    width: u32,
    height: u32,
    format: TextureFormat,
    bytes: &[u8],
    label: &str,
) -> Result<TextureHandle, String> {
    let mut desc = TextureDesc::new(
        Extent2D::new(width.max(1), height.max(1)),
        format,
        TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
    );
    desc.label = label.to_string();
    let handle = runtime
        .create_texture(desc)
        .map_err(|error| error.to_string())?;
    let row_pitch = width.max(1).saturating_mul(4);
    if let Err(error) = runtime.device().upload_texture(handle, bytes, row_pitch) {
        let _ = runtime.destroy_texture(handle);
        return Err(error.to_string());
    }
    if let Some(previous) = current.replace(handle) {
        let _ = runtime.destroy_texture(previous);
    }
    Ok(handle)
}

impl<D: RenderDevice> GameRenderer<D> {
    const GUI_BUILD_INTERVAL: Duration = Duration::from_micros(16_667);

    pub fn notify_resize(&mut self, width: u32, height: u32) {
        self.gui.viewport = [width, height];
        self.sky.viewport = [width, height];
        self.gui.invalidate();
        if self.runtime.capabilities().supports_present {
            let config = SurfaceConfig::new(
                Extent2D::new(width.max(1), height.max(1)),
                TextureFormat::B8G8R8A8Srgb,
            );
            if let Err(error) = self.runtime.device().configure_surface(config) {
                log::debug!("deferred Vulkan surface resize: {error}");
            }
        }
    }

    pub fn set_gui_mouse_pos(&mut self, x: f32, y: f32) {
        self.gui.set_mouse_pos(x, y);
    }

    pub fn invalidate_gui(&mut self) {
        self.gui.invalidate();
    }

    pub fn gui_hit_test(&self, mouse_x: f32, mouse_y: f32) -> Option<u32> {
        self.gui.hit_test(mouse_x, mouse_y)
    }

    pub fn gui_hit_rect(&self, id: u32) -> Option<&crate::render::gui::ButtonHit> {
        self.gui.hit_rect(id)
    }

    pub fn gui_directional_hit(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        direction: [f32; 2],
    ) -> Option<crate::render::gui::ButtonHit> {
        self.gui.directional_hit(mouse_x, mouse_y, direction)
    }

    pub fn creative_visible_entries(&self) -> Vec<CreativeItemEntry> {
        let mut entries = super::gui_renderer::creative_tab_entries(self.gui.creative_tab);
        if self.gui.creative_tab == super::gui_renderer::CREATIVE_TAB_SEARCH {
            let query = self.gui.creative_search.trim().to_lowercase();
            if !query.is_empty() {
                entries.retain(|entry| {
                    entry.item_id.to_string().contains(&query)
                        || crate::render::item_icons::item_icon_path(entry.item_id, entry.damage)
                            .is_some_and(|path| path.to_lowercase().contains(&query))
                });
            }
        }
        entries
    }

    pub fn upload_world_partial(&mut self, meshes: &[ChunkMesh]) {
        self.world.update_meshes(meshes);
    }

    pub fn upload_world(&mut self, meshes: &[ChunkMesh]) {
        self.world.replace_meshes(meshes);
    }

    pub fn set_particles(
        &mut self,
        particles: &[crate::client::particles::Particle],
        generation: u64,
    ) {
        self.world.set_particles(particles, generation);
        self.entities.set_particles(particles, generation);
        self.frame.particle_count = particles.len();
    }

    pub fn trigger_hand_swing(&mut self) {
        self.hud.hand_swing_timer = 0.3;
        self.hud.hand_swing_progress = 0.0;
        self.hud.hand_animation_last_update = Some(std::time::Instant::now());
    }

    pub fn reset_equipped_progress(&mut self) {
        self.hud.hand_equip_progress = 0.0;
    }

    pub fn update_hand_state(&mut self, item_id: u16, item_damage: u16, nbt: Option<Vec<u8>>) {
        let now = std::time::Instant::now();
        let dt = self
            .hud
            .hand_animation_last_update
            .replace(now)
            .map_or(1.0 / 60.0, |last| {
                now.duration_since(last).as_secs_f32().min(0.1)
            });

        if self.hud.hand_swing_timer > 0.0 {
            self.hud.hand_swing_timer = (self.hud.hand_swing_timer - dt).max(0.0);
            self.hud.hand_swing_progress = if self.hud.hand_swing_timer > 0.0 {
                1.0 - self.hud.hand_swing_timer / 0.3
            } else {
                0.0
            };
        } else {
            self.hud.hand_swing_progress = 0.0;
        }

        let same_item = self.hud.hand_item_id == item_id
            && (item_id == crate::world::item::Item::Bow.to_id()
                || self.hud.hand_item_damage == item_damage);
        let target = if same_item { 1.0 } else { 0.0 };
        let step = 8.0 * dt;
        if self.hud.hand_equip_progress < target {
            self.hud.hand_equip_progress = (self.hud.hand_equip_progress + step).min(target);
        } else {
            self.hud.hand_equip_progress = (self.hud.hand_equip_progress - step).max(target);
        }

        if !same_item && self.hud.hand_equip_progress <= 0.1 {
            self.hud.hand_item_id = item_id;
            self.hud.hand_item_damage = item_damage;
            self.hud.hand_item_nbt = nbt;
        }
    }

    pub fn update_skin_gpu(&mut self) {
        self.entities.local_skin_upload_pending = true;
    }

    pub fn upload_cape_to_atlas(&mut self, pixels: &[u8]) {
        self.entities.cape_pixels = Some(pixels.to_vec());
        self.entities.cape_upload_pending = true;
    }

    pub fn schedule_resource_reload(
        &mut self,
        block_atlas: TextureAtlas,
        entity_atlas: EntityTextureAtlas,
        item_atlas: Vec<u8>,
        resolver: &mut AssetResolver,
    ) {
        let gui_pipeline = self.gui.pipeline();
        let world_pipeline = self.world.pipeline();
        let world_transparent_pipeline = self.world.transparent_pipeline();
        let entity_pipeline = self.entities.pipeline();
        let entity_transparent_pipeline = self.entities.transparent_pipeline();
        let entity_first_person_pipeline = self.entities.first_person_pipeline();
        self.gui.destroy_gpu(&mut self.runtime);
        self.world.destroy_gpu(&mut self.runtime);
        self.entities.destroy_gpu(&mut self.runtime);
        self.gui.destroy_owned_textures(&mut self.runtime);

        let block = replace_uploaded_texture(
            &mut self.runtime,
            &mut self.world.block_texture,
            block_atlas.width,
            block_atlas.height,
            TextureFormat::R8G8B8A8Srgb,
            &block_atlas.pixels,
            "game.block-atlas",
        );
        if let Err(error) = block {
            log::error!("failed to upload block atlas through RHI: {error}");
        }

        let normal = replace_uploaded_texture(
            &mut self.runtime,
            &mut self.world.normal_texture,
            block_atlas.width,
            block_atlas.height,
            TextureFormat::R8G8B8A8Unorm,
            &block_atlas.normal_pixels,
            "game.block-normal-atlas",
        );
        if let Err(error) = normal {
            log::error!("failed to upload normal atlas through RHI: {error}");
        }

        let specular = replace_uploaded_texture(
            &mut self.runtime,
            &mut self.world.specular_texture,
            block_atlas.width,
            block_atlas.height,
            TextureFormat::R8G8B8A8Unorm,
            &block_atlas.specular_pixels,
            "game.block-specular-atlas",
        );
        if let Err(error) = specular {
            log::error!("failed to upload specular atlas through RHI: {error}");
        }

        self.block_atlas = Some(block_atlas);

        if let Some(texture) = self.world.block_texture {
            self.gui
                .set_texture(GuiTextureKind::Blocks, Some(texture), false);
        }

        let item = GuiRenderer::upload_texture(
            &mut self.runtime,
            ITEM_ATLAS_W,
            ITEM_ATLAS_H,
            &item_atlas,
            "game.item-atlas",
        );
        match item {
            Ok(texture) => {
                self.entities.item_texture = Some(texture);
                self.gui
                    .set_texture(GuiTextureKind::Items, Some(texture), true);
            }
            Err(error) => log::error!("failed to upload item atlas through RHI: {error}"),
        }

        for (kind, path, label) in [
            (
                GuiTextureKind::Widgets,
                "minecraft/textures/gui/widgets.png",
                "game.gui.widgets",
            ),
            (
                GuiTextureKind::Inventory,
                "minecraft/textures/gui/container/inventory.png",
                "game.gui.inventory",
            ),
            (
                GuiTextureKind::Icons,
                "minecraft/textures/gui/icons.png",
                "game.gui.icons",
            ),
            (
                GuiTextureKind::OptionsBackground,
                "minecraft/textures/gui/options_background.png",
                "game.gui.options-background",
            ),
            (
                GuiTextureKind::Underwater,
                "minecraft/textures/misc/underwater.png",
                "game.gui.underwater",
            ),
        ] {
            match decode_gui_image(resolver, path)
                .and_then(|image| upload_gui_image(&mut self.runtime, image, label))
            {
                Ok(texture) => self.gui.set_texture(kind, Some(texture), true),
                Err(error) => log::error!("{error}"),
            }
        }

        let container_paths = [
            "minecraft/textures/gui/container/generic_54.png",
            "minecraft/textures/gui/container/crafting_table.png",
            "minecraft/textures/gui/container/furnace.png",
            "minecraft/textures/gui/container/dispenser.png",
            "minecraft/textures/gui/container/hopper.png",
            "minecraft/textures/gui/container/brewing_stand.png",
            "minecraft/textures/gui/container/enchanting_table.png",
            "minecraft/textures/gui/container/anvil.png",
            "minecraft/textures/gui/container/beacon.png",
            "minecraft/textures/gui/container/villager.png",
            "minecraft/textures/gui/container/horse.png",
        ];
        match build_gui_grid_atlas(resolver, &container_paths, 4, 3).and_then(|image| {
            upload_gui_image(&mut self.runtime, image, "game.gui.container-atlas")
        }) {
            Ok(texture) => self
                .gui
                .set_texture(GuiTextureKind::Generic54, Some(texture), true),
            Err(error) => log::error!("failed to build GUI container atlas: {error}"),
        }

        let creative_paths = [
            "minecraft/textures/gui/container/creative_inventory/tabs.png",
            "minecraft/textures/gui/container/creative_inventory/tab_items.png",
            "minecraft/textures/gui/container/creative_inventory/tab_inventory.png",
            "minecraft/textures/gui/container/creative_inventory/tab_item_search.png",
        ];
        match build_gui_grid_atlas(resolver, &creative_paths, 2, 2)
            .and_then(|image| upload_gui_image(&mut self.runtime, image, "game.gui.creative-atlas"))
        {
            Ok(texture) => self
                .gui
                .set_texture(GuiTextureKind::Creative, Some(texture), true),
            Err(error) => log::error!("failed to build creative GUI atlas: {error}"),
        }

        let entity = replace_uploaded_texture(
            &mut self.runtime,
            &mut self.entities.entity_texture,
            ENTITY_ATLAS_SIZE,
            ENTITY_ATLAS_SIZE,
            TextureFormat::R8G8B8A8Srgb,
            &entity_atlas.pixels,
            "game.entity-atlas",
        );
        if let Err(error) = entity {
            log::error!("failed to upload entity atlas through RHI: {error}");
        }
        self.entities.entity_atlas = Some(entity_atlas);
        self.entities.block_texture = self.world.block_texture;
        self.entities.synced_player_skin_content_hash = None;
        self.entities.local_skin_upload_pending = true;
        self.entities.cape_upload_pending = false;

        if let (Some(pipeline), Some(transparent_pipeline)) =
            (world_pipeline, world_transparent_pipeline)
        {
            if let Err(error) =
                self.world
                    .configure_gpu(&mut self.runtime, pipeline, transparent_pipeline)
            {
                log::error!("failed to rebuild world RHI resources: {error}");
            }
        }
        if let (Some(pipeline), Some(transparent_pipeline), Some(first_person_pipeline)) = (
            entity_pipeline,
            entity_transparent_pipeline,
            entity_first_person_pipeline,
        ) {
            if let Err(error) = self.entities.configure_gpu(
                &mut self.runtime,
                pipeline,
                transparent_pipeline,
                first_person_pipeline,
            ) {
                log::error!("failed to rebuild entity RHI resources: {error}");
            }
        }
        if let Some(pipeline) = gui_pipeline {
            if let Err(error) = self.gui.configure_gpu(&mut self.runtime, pipeline) {
                log::error!("failed to rebuild GUI RHI resources: {error}");
            }
        }
        self.invalidate_gui();
    }

    /// Advance atlas animations once per client tick and upload only the
    /// sprite rectangles whose selected frame changed.
    pub fn tick_texture_animations(&mut self, context: TextureAnimationContext) {
        let Some(atlas) = self.block_atlas.as_mut() else {
            return;
        };
        let mut bytes = Vec::new();
        let mut uploads = Vec::new();
        atlas.animate_tick_with_context_into(context, &mut bytes, &mut uploads);
        if uploads.is_empty() {
            return;
        }

        let block_texture = self.world.block_texture;
        let normal_texture = self.world.normal_texture;
        let specular_texture = self.world.specular_texture;
        let item_texture = self.gui.texture(GuiTextureKind::Items);
        let mut item_updates = Vec::new();
        for upload in &uploads {
            let Some(animation) = atlas.animated.get(&upload.atlas_index) else {
                continue;
            };
            let Some(name) = atlas
                .tiles
                .get(&upload.atlas_index)
                .map(|tile| tile.texture_name.as_str())
            else {
                continue;
            };
            let Some(icon_index) = crate::render::item_icons::item_icon_entries()
                .iter()
                .position(|entry| *entry == name)
            else {
                continue;
            };
            let Some(frame) = animation.frames.get(upload.frame_index as usize) else {
                continue;
            };
            let Some(image) =
                image::RgbaImage::from_raw(animation.width, animation.height, frame.clone())
            else {
                continue;
            };
            let resized = image::imageops::resize(
                &image,
                crate::render::item_icons::ITEM_ATLAS_CELL,
                crate::render::item_icons::ITEM_ATLAS_CELL,
                image::imageops::FilterType::Nearest,
            );
            let x = icon_index as u32 % crate::render::item_icons::ITEM_ATLAS_COLS
                * crate::render::item_icons::ITEM_ATLAS_CELL;
            let y = icon_index as u32 / crate::render::item_icons::ITEM_ATLAS_COLS
                * crate::render::item_icons::ITEM_ATLAS_CELL;
            item_updates.push(([x, y], resized.into_raw()));
        }

        for upload in uploads {
            let byte_len = upload.width as usize * upload.height as usize * 4;
            let base = upload.buffer_offset as usize;
            if let (Some(texture), Some(data)) = (
                block_texture,
                bytes.get(base..base.saturating_add(byte_len)),
            ) {
                if let Err(error) = self.runtime.device().upload_texture_region(
                    texture,
                    [upload.pixel_x, upload.pixel_y],
                    Extent2D::new(upload.width, upload.height),
                    data,
                    upload.width * 4,
                ) {
                    log::warn!("failed to upload animated atlas region: {error}");
                }
            }
            for (texture, offset, label) in [
                (normal_texture, upload.normal_buffer_offset, "normal"),
                (specular_texture, upload.specular_buffer_offset, "specular"),
            ] {
                let (Some(texture), Some(offset)) = (texture, offset) else {
                    continue;
                };
                let offset = offset as usize;
                let Some(data) = bytes.get(offset..offset.saturating_add(byte_len)) else {
                    continue;
                };
                if let Err(error) = self.runtime.device().upload_texture_region(
                    texture,
                    [upload.pixel_x, upload.pixel_y],
                    Extent2D::new(upload.width, upload.height),
                    data,
                    upload.width * 4,
                ) {
                    log::warn!("failed to upload animated {label} atlas region: {error}");
                }
            }
        }

        if let Some(texture) = item_texture {
            let extent = Extent2D::new(
                crate::render::item_icons::ITEM_ATLAS_CELL,
                crate::render::item_icons::ITEM_ATLAS_CELL,
            );
            for (origin, pixels) in item_updates {
                if let Err(error) = self.runtime.device().upload_texture_region(
                    texture,
                    origin,
                    extent,
                    &pixels,
                    extent.width * 4,
                ) {
                    log::warn!("failed to upload animated item icon: {error}");
                }
            }
        }
    }

    pub fn reload_custom_sky(&mut self, pixels: &[u8], width: u32, height: u32, data: CustomSky) {
        let texture = replace_uploaded_texture(
            &mut self.runtime,
            &mut self.sky.custom_sky_texture,
            width,
            height,
            TextureFormat::R8G8B8A8Srgb,
            pixels,
            "game.custom-sky",
        )
        .ok();
        self.sky.set_custom_sky(data, texture);
    }

    pub fn load_optifine_shader_pack(
        &mut self,
        source_name: &str,
        overrides: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        for handle in self.shaders.handles.drain(..) {
            let _ = self.runtime.destroy_shader(handle);
        }
        self.shaders.load(source_name, overrides)?;

        let mut handles = Vec::new();
        for program in &mut self.shaders.programs {
            for stage in &mut program.stages {
                let desc = ShaderDesc::new(
                    stage.stage.stage.rhi_stage(),
                    ShaderCode::Source(stage.stage.source.clone()),
                )
                .with_interface(stage.stage.rhi_interface());
                match self.runtime.create_shader(desc) {
                    Ok(handle) => {
                        stage.handle = Some(handle);
                        handles.push(handle);
                    }
                    Err(error) => {
                        for handle in handles {
                            let _ = self.runtime.destroy_shader(handle);
                        }
                        stage.handle = None;
                        self.shaders.handles.clear();
                        return Err(format!(
                            "backend shader creation failed for {}: {error}",
                            stage.stage.source_path
                        ));
                    }
                }
            }
        }
        self.shaders.handles = handles;
        self.screens.selected_shader_pack = Some(source_name.to_string());
        Ok(())
    }

    pub fn unload_optifine_shader_pack(&mut self) {
        for handle in self.shaders.handles.drain(..) {
            let _ = self.runtime.destroy_shader(handle);
        }
        self.shaders.unload();
        self.screens.selected_shader_pack = None;
    }

    pub fn draw_frame(&mut self, camera: &Camera, _menu: u32, underwater: bool, in_world: bool) {
        // These are frame-local diagnostics. Reset them before any early
        // return so the debug HUD never displays timing data from an older
        // frame after a swapchain/acquire error.
        self.frame.frame_gpu_us = 0;
        self.frame.frame_cpu_us = 0;
        self.frame.frame_acquire_us = 0;
        self.frame.frame_command_us = 0;
        self.frame.frame_submit_us = 0;
        self.frame.frame_present_us = 0;
        self.world_background = in_world;
        self.world.underwater = underwater;
        self.world.underwater_yaw = camera.yaw;
        self.world.underwater_pitch = camera.pitch;

        let [display_width, display_height] = self.gui.viewport;
        let (display_width, display_height) = if display_width == 0 || display_height == 0 {
            (1280, 720)
        } else {
            (display_width, display_height)
        };

        // Acquire before writing any per-frame upload buffers. Both real
        // backends wait for the reusable frame slot here; doing it after the
        // world/entity/GUI sync allowed the CPU to overwrite a uniform buffer
        // while the previous submission was still reading it.
        let acquire_started = Instant::now();
        let surface_frame = if self.runtime.capabilities().supports_present {
            let mut config = SurfaceConfig::new(
                Extent2D::new(display_width, display_height),
                TextureFormat::B8G8R8A8Srgb,
            );
            // The client-side framerate policy is handled by the event loop;
            // FIFO here would cap every 1000-FPS/unlimited configuration at
            // the monitor refresh rate before the software limiter runs.
            config.vsync = false;
            if let Err(error) = self.runtime.device().configure_surface(config) {
                log::error!("failed to configure RHI surface: {error}");
                return;
            }
            match self.runtime.device().acquire_surface_frame() {
                Ok(frame) => Some(frame),
                Err(error) => {
                    log::error!("failed to acquire RHI surface frame: {error}");
                    return;
                }
            }
        } else {
            None
        };
        self.frame.frame_acquire_us = acquire_started.elapsed().as_micros() as u64;
        // The acquire path is where the reusable frame-slot fence is waited;
        // expose that wait in the existing "fence" diagnostic field.
        self.frame.frame_gpu_us = self.frame.frame_acquire_us;

        let gui_started = Instant::now();
        let mouse = [self.gui.mouse_x, self.gui.mouse_y];
        let pointer_changed = self.gui_build_mouse != mouse;
        let gui_build_due = self.gui_build_at.is_none_or(|last| {
            last.elapsed() >= Self::GUI_BUILD_INTERVAL && (_menu == 1 || pointer_changed)
        });
        // Mouse movement only changes hover highlighting. Sample it at the
        // existing 60 Hz build cadence instead of bypassing that cadence and
        // rebuilding the complete HUD/menu on every input event. Static menus
        // do not rebuild at all until their pointer or state changes.
        let gui_state_changed = self.gui_build_at.is_none()
            || self.gui_build_menu != _menu
            || self.gui_build_underwater != underwater
            || self.gui_build_in_world != in_world
            || self.gui.dirty;
        if gui_build_due || gui_state_changed {
            self.screens
                .rebuild(_menu, in_world, &self.settings, &mut self.gui);
            if _menu == 1 {
                self.hud.rebuild(
                    &mut self.gui,
                    &self.settings,
                    &self.frame,
                    &self.world,
                    &self.screens,
                    &self.entities,
                );
            }
            self.gui_build_at = Some(Instant::now());
            self.gui_build_menu = _menu;
            self.gui_build_underwater = underwater;
            self.gui_build_in_world = in_world;
            self.gui_build_mouse = mouse;
        }
        let world_started = Instant::now();
        if in_world {
            self.sky.update_environment(
                self.world.day_time,
                self.settings.render_distance,
                self.world.dimension,
            );
            self.world.set_environment(self.sky.environment());
            self.entities.set_environment(self.sky.environment());
            if let Err(error) = self.world.sync_gpu(&mut self.runtime, camera) {
                log::error!("failed to upload world batches through RHI: {error}");
            }
            self.frame.frame_mesh_us = world_started.elapsed().as_micros() as u64;
            self.sky.viewport = [display_width, display_height];
            if let Err(error) = self.sky.sync_gpu(&mut self.runtime, camera) {
                log::error!("failed to update sky through RHI: {error}");
            }
            let entity_started = Instant::now();
            self.entities.set_block_overlay(
                &self.world.block_selection_boxes,
                self.world.dig_progress,
                self.world.dig_position,
            );
            self.entities.daylight_factor = self.world.sky_brightness_cached;
            if let Err(error) = self.entities.sync_gpu(
                &mut self.runtime,
                camera,
                &mut self.gui.font,
                &self.hud,
                &self.settings,
                underwater,
            ) {
                log::error!("failed to update entities through RHI: {error}");
            }
            self.frame.frame_entity_us = entity_started.elapsed().as_micros() as u64;
        } else {
            self.frame.frame_mesh_us = 0;
            self.frame.frame_entity_us = 0;
        }

        // Nametags are projected after entity interpolation and before GUI
        // upload, so they follow the current camera without rebuilding the
        // cached HUD/menu batches.  They still submit through the GUI RHI
        // pipeline and remain backend-independent.
        self.gui.update_nametags(
            camera,
            &self.entities.entity_billboards,
            [display_width, display_height],
            in_world,
        );
        if let Err(error) = self.gui.sync_gpu(&mut self.runtime) {
            log::error!("failed to upload GUI batches through RHI: {error}");
        }
        self.frame.frame_gui_us = gui_started.elapsed().as_micros() as u64;

        let command_started = Instant::now();
        let prepared = self.prepare_frame(FrameInputs {
            render_width: display_width,
            render_height: display_height,
            display_width,
            display_height,
            // The RHI graph currently has no FSR/post-process executor. Do
            // not route HDR into an uninitialised post-process target when a
            // saved config has FSR enabled; keep the valid HDR -> tone map
            // path until that executor is installed.
            enable_post_processing: false,
            targets: renderer_api::core::FrameTargetHandles {
                swapchain: surface_frame.map(|frame| frame.texture),
                ..Default::default()
            },
        });
        if let Err(error) = prepared {
            log::error!("failed to prepare RHI frame: {error}");
            if let Some(frame) = surface_frame {
                let _ = self.runtime.device().present(frame);
            }
            return;
        }
        self.submit();
        self.frame.frame_command_us = command_started.elapsed().as_micros() as u64;
        let submit_started = Instant::now();
        if let Err(error) = self.end_frame() {
            log::error!("failed to submit RHI frame: {error}");
        }
        self.frame.frame_submit_us = submit_started.elapsed().as_micros() as u64;
        let present_started = Instant::now();
        if let Some(frame) = surface_frame {
            if let Err(error) = self.runtime.device().present(frame) {
                log::error!("failed to present RHI surface frame: {error}");
            }
        }
        self.frame.frame_present_us = present_started.elapsed().as_micros() as u64;
    }
}
