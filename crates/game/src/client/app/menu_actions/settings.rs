use crate::audio::AudioBackend;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use crate::ui::button_ids as btn;
use winit::event_loop::ActiveEventLoop;

use super::{
    adjusted_config_value, config_value_label, next_framerate, previous_framerate,
    slider_to_framerate, slider_value_from_mouse,
};
use crate::client::app::{
    scan_resource_packs, App, PendingShaderPackLoad, ShaderPackLoadReason, MC_PACK_FORMAT,
};
impl App {
    pub(crate) fn open_options(&mut self) {
        let previous = std::mem::replace(&mut self.state, GameState::MainMenu);
        self.state = GameState::Options {
            previous: Box::new(previous),
        };
    }

    pub(crate) fn open_modding(&mut self) {
        let previous = std::mem::replace(&mut self.state, GameState::MainMenu);
        self.state = GameState::Modding {
            previous: Box::new(previous),
        };
    }

    pub(crate) fn open_selected_mod_config(&mut self) {
        let selected = self
            .scripts
            .loaded_mods()
            .into_iter()
            .nth(self.modding_selected);
        let Some(mod_info) = selected else {
            self.modding_status = "Select a mod first".to_string();
            return;
        };
        if mod_info.config_entries == 0 {
            self.modding_status =
                format!("{} does not expose configurable settings", mod_info.name);
            return;
        }

        let previous = std::mem::replace(&mut self.state, GameState::MainMenu);
        self.state = GameState::ModConfig {
            previous: Box::new(previous),
            mod_id: mod_info.id,
        };
        self.mod_config_selected = 0;
        self.mod_config_status.clear();
        if let Some(renderer) = &mut self.renderer {
            renderer.screens.mod_config_scroll = 0;
        }
    }

    pub(crate) fn active_mod_config_id(&self) -> Option<String> {
        match &self.state {
            GameState::ModConfig { mod_id, .. } => Some(mod_id.clone()),
            _ => None,
        }
    }

    pub(crate) fn adjust_mod_config(&mut self, index: usize, direction: i32) {
        let Some(mod_id) = self.active_mod_config_id() else {
            return;
        };
        let Ok(entries) = self.scripts.config_entries(&mod_id) else {
            self.mod_config_status = "The selected mod is no longer available".to_string();
            return;
        };
        let Some(entry) = entries.get(index) else {
            return;
        };
        self.mod_config_selected = index;

        let Some(next) = adjusted_config_value(&entry.kind, &entry.value, direction) else {
            self.mod_config_status = format!("{} has an invalid configuration value", entry.label);
            return;
        };
        let value_label = config_value_label(&entry.kind, &next);
        self.mod_config_status = match self.scripts.set_config_value(&mod_id, &entry.key, next) {
            Ok(()) => format!("Set {} to {value_label}", entry.label),
            Err(error) => format!("Could not change {}: {error}", entry.label),
        };
    }

    pub(crate) fn reset_mod_config(&mut self, index: usize) {
        let Some(mod_id) = self.active_mod_config_id() else {
            return;
        };
        let Ok(entries) = self.scripts.config_entries(&mod_id) else {
            self.mod_config_status = "The selected mod is no longer available".to_string();
            return;
        };
        let Some(entry) = entries.get(index) else {
            return;
        };
        self.mod_config_selected = index;
        self.mod_config_status =
            match self
                .scripts
                .set_config_value(&mod_id, &entry.key, entry.default_value.clone())
            {
                Ok(()) => format!("Reset {}", entry.label),
                Err(error) => format!("Could not reset {}: {error}", entry.label),
            };
    }

    pub(crate) fn reset_all_mod_config(&mut self) {
        let Some(mod_id) = self.active_mod_config_id() else {
            return;
        };
        let Ok(entries) = self.scripts.config_entries(&mod_id) else {
            self.mod_config_status = "The selected mod is no longer available".to_string();
            return;
        };
        let mut reset = 0usize;
        for entry in entries {
            if entry.value == entry.default_value {
                continue;
            }
            if let Err(error) =
                self.scripts
                    .set_config_value(&mod_id, &entry.key, entry.default_value)
            {
                self.mod_config_status = format!("Could not reset settings: {error}");
                return;
            }
            reset += 1;
        }
        self.mod_config_status = format!("Reset {reset} setting(s) to defaults");
    }

    pub(crate) fn push_subscreen(&mut self, placeholder: GameState) {
        let previous = std::mem::replace(&mut self.state, GameState::MainMenu);
        self.state = match placeholder {
            GameState::VideoSettings { .. } => GameState::VideoSettings {
                previous: Box::new(previous),
            },
            GameState::Controls { .. } => GameState::Controls {
                previous: Box::new(previous),
            },
            GameState::SkinCustomization { .. } => GameState::SkinCustomization {
                previous: Box::new(previous),
            },
            GameState::Language { .. } => GameState::Language {
                previous: Box::new(previous),
            },
            GameState::AudioSettings { .. } => GameState::AudioSettings {
                previous: Box::new(previous),
            },
            GameState::ResourcePacks { .. } => GameState::ResourcePacks {
                previous: Box::new(previous),
            },
            GameState::ShaderPacks { .. } => GameState::ShaderPacks {
                previous: Box::new(previous),
            },
            GameState::ShaderOptions { .. } => GameState::ShaderOptions {
                previous: Box::new(previous),
            },
            GameState::ChatSettings { .. } => GameState::ChatSettings {
                previous: Box::new(previous),
            },
            _ => previous,
        };
    }

    pub(crate) fn set_cursor_captured(&self, captured: bool) {
        if let Some(window) = &self.window {
            let mode = if captured {
                winit::window::CursorGrabMode::Locked
            } else {
                winit::window::CursorGrabMode::None
            };
            window.set_cursor_grab(mode).ok();
            window.set_cursor_visible(!captured);
        }
    }

    pub(crate) fn adjust_volume(&mut self, category: crate::audio::SoundCategory, delta: f32) {
        let target = match category {
            crate::audio::SoundCategory::Master => &mut self.config.master_volume,
            crate::audio::SoundCategory::Music => &mut self.config.music_volume,
            crate::audio::SoundCategory::Blocks => &mut self.config.blocks_volume,
            crate::audio::SoundCategory::Weather => &mut self.config.weather_volume,
            crate::audio::SoundCategory::Hostile => &mut self.config.hostile_volume,
            crate::audio::SoundCategory::Friendly => &mut self.config.friendly_volume,
            crate::audio::SoundCategory::Players => &mut self.config.players_volume,
            crate::audio::SoundCategory::Ambient => &mut self.config.ambient_volume,
            crate::audio::SoundCategory::Ui => &mut self.config.ui_volume,
        };
        *target = (*target + delta).clamp(0.0, 1.0);
        self.audio.set_volume(category, *target);
        self.config.save_default();
    }

    pub(crate) fn set_volume_absolute(
        &mut self,
        category: crate::audio::SoundCategory,
        value: f32,
    ) {
        let target = match category {
            crate::audio::SoundCategory::Master => &mut self.config.master_volume,
            crate::audio::SoundCategory::Music => &mut self.config.music_volume,
            crate::audio::SoundCategory::Blocks => &mut self.config.blocks_volume,
            crate::audio::SoundCategory::Weather => &mut self.config.weather_volume,
            crate::audio::SoundCategory::Hostile => &mut self.config.hostile_volume,
            crate::audio::SoundCategory::Friendly => &mut self.config.friendly_volume,
            crate::audio::SoundCategory::Players => &mut self.config.players_volume,
            crate::audio::SoundCategory::Ambient => &mut self.config.ambient_volume,
            crate::audio::SoundCategory::Ui => &mut self.config.ui_volume,
        };
        *target = value.clamp(0.0, 1.0);
        self.audio.set_volume(category, *target);
        self.config.save_default();
    }

    pub(crate) fn sync_chat_settings(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            renderer.hud.chat_width = self.config.chat_width;
            renderer.hud.chat_height = self.config.chat_height;
            renderer.hud.chat_background = self.config.chat_background;
            renderer.hud.chat_overlay = self.config.chat_overlay;
            renderer.hud.chat_player_avatars = self.config.chat_player_avatars;
            renderer.hud.tab_player_avatars = self.config.tab_player_avatars;
        }
    }

    pub(crate) fn save_and_sync_client_settings(&mut self) {
        self.config.save_default();
        self.session.locale = self.config.language.clone();
        self.session.view_distance = self.config.render_distance;
        self.session.skin_parts = self.config.skin_parts;
        crate::client::network::send_client_settings(
            &self.connection,
            &self.session.locale,
            self.session.view_distance,
            self.session.skin_parts,
        );
    }

    pub(crate) fn toggle_skin_part(&mut self, mask: u8) {
        self.config.skin_parts ^= mask;
        self.config.skin_parts &= 0x7f;
        self.save_and_sync_client_settings();
    }

    pub(crate) fn accept_resource_pack(&mut self) {
        let Some(pack) = &mut self.session.resource_pack else {
            return;
        };
        let hash = pack.hash.clone();
        crate::client::network::send_resource_pack_status(&self.connection, &hash, 3);
        pack.status = "accepted".to_string();
        self.session
            .push_system_line(self.ui.t("rustcraft.resourcePack.acceptedPending"));
        crate::client::network::send_resource_pack_status(&self.connection, &hash, 0);
        if let Some(pack) = &mut self.session.resource_pack {
            pack.status = "loaded".to_string();
        }
        if matches!(self.state, GameState::Playing) && !self.chat_open && !self.inventory_open {
            self.mouse_captured = true;
            self.set_cursor_captured(true);
        }
    }

    pub(crate) fn decline_resource_pack(&mut self) {
        let Some(pack) = &mut self.session.resource_pack else {
            return;
        };
        let hash = pack.hash.clone();
        crate::client::network::send_resource_pack_status(&self.connection, &hash, 1);
        pack.status = "declined".to_string();
        self.session
            .push_system_line(self.ui.t("rustcraft.resourcePack.declined"));
        if matches!(self.state, GameState::Playing) && !self.chat_open && !self.inventory_open {
            self.mouse_captured = true;
            self.set_cursor_captured(true);
        }
    }

    pub(crate) fn handle_dropped_file(&mut self, path: std::path::PathBuf) {
        let shader_pack = matches!(self.state, GameState::ShaderPacks { .. });
        if !shader_pack && !matches!(self.state, GameState::ResourcePacks { .. }) {
            return;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("zip") {
            return;
        }
        let rp_dir = std::path::Path::new(if shader_pack {
            crate::render::shader_pack::SHADER_PACK_DIR
        } else {
            "resourcepacks"
        });
        let _ = std::fs::create_dir_all(rp_dir);
        let dest = rp_dir.join(path.file_name().unwrap_or_default());
        if let Err(e) = std::fs::copy(&path, &dest) {
            log::error!(
                "failed to copy dropped resource pack from '{}' to '{}': {}",
                path.display(),
                dest.display(),
                e
            );
            return;
        }
        if shader_pack {
            self.scan_and_update_shader_packs();
        } else if let Some(name) = dest.file_stem().and_then(|s| s.to_str()) {
            let name = name.to_string();
            if !self.resource_pack_list.iter().any(|p| p.name == name) {
                self.resource_pack_list
                    .push(crate::client::app::ResourcePackInfo {
                        name,
                        enabled: false,
                        is_default: false,
                        description: String::new(),
                        pack_format: 0,
                        icon: None,
                    });
            }
            self.scan_and_update_resource_packs();
        }
    }

    /// Reload all game assets (textures, models, sounds, language) from current resource packs.
    pub(crate) fn reload_assets(&mut self) {
        // Running mesh jobs contain atlas UVs captured before this reload.
        // Invalidate them before publishing the replacement texture layout.
        self.world.invalidate_mesh_jobs_for_resource_reload();
        let enabled = self.config.enabled_resource_packs.clone();
        let mut resolver = crate::assets::resolver::AssetResolver::with_resource_packs(
            "assets/minecraft",
            "resourcepacks",
            &enabled,
        );
        match crate::assets::resource_pack::ResourcePack::load_with_resource_packs(
            "assets",
            "1.8",
            &mut resolver,
        ) {
            Ok(pack) => self.audio.reload_assets(pack.index, pack.sounds),
            Err(error) => log::error!("sound asset reload failed: {error}"),
        }
        // Rebuild block texture atlas
        let tex_atlas = crate::assets::texture::TextureAtlas::load_with_resolver(&mut resolver);
        crate::assets::texture::init_texture_map(&tex_atlas);

        // Rebuild block model cache
        let mut model_registry = crate::assets::model::ModelRegistry::new();
        model_registry.load_with_resolver(&mut resolver);
        model_registry.texture_map = tex_atlas.name_to_index.clone();
        let model_cache = crate::world::block_models::BlockModelCache::build(
            &mut model_registry,
            tex_atlas.name_to_index.clone(),
        );
        crate::world::block_models::BlockModelCache::init(model_cache);

        // Rebuild entity texture atlas
        let entity_atlas =
            crate::render::entity::atlas::EntityTextureAtlas::load_with_resolver(&mut resolver);
        let item_atlas = crate::render::item_icons::build_item_icon_atlas(&mut resolver);
        crate::render::item_icons::precompute_item_meshes_with_resolver(&mut resolver);

        // Push reload to renderer
        if let Some(renderer) = &mut self.renderer {
            renderer.schedule_resource_reload(tex_atlas, entity_atlas, item_atlas, &mut resolver);
            let meshes = self.world.build_all_meshes();
            renderer.upload_world(&meshes);

            // Reload custom sky from mcpatcher
            if let Some(mut custom_sky) =
                crate::render::custom_sky::CustomSky::load(&mut resolver, 0)
            {
                let pixels = custom_sky.layers.first().map(|l| l.pixels.clone());
                let (w, h) = custom_sky
                    .layers
                    .first()
                    .map_or((1, 1), |l| (l.width, l.height));
                if let Some(ref pixels) = pixels {
                    renderer.reload_custom_sky(pixels, w, h, custom_sky);
                }
            }
        }

        // Reload language and UI text
        self.ui = crate::ui::UiState::new(&self.config.language);

        log::info!(
            "asset reload complete: enabled_resource_packs={}",
            enabled.len()
        );
    }

    pub(super) fn scan_and_update_resource_packs(&mut self) {
        let enabled = self.config.enabled_resource_packs.clone();
        self.resource_pack_list = scan_resource_packs(&enabled);
        if let Some(renderer) = &mut self.renderer {
            renderer.screens.available_resource_packs = self
                .resource_pack_list
                .iter()
                .filter(|p| !p.enabled)
                .cloned()
                .collect();
            renderer.screens.selected_resource_packs = enabled
                .iter()
                .filter_map(|name| {
                    self.resource_pack_list
                        .iter()
                        .find(|pack| pack.name == *name)
                        .cloned()
                })
                .collect();
            renderer
                .screens
                .selected_resource_packs
                .push(crate::client::app::ResourcePackInfo {
                    name: "Default".to_string(),
                    enabled: true,
                    is_default: true,
                    description: "The default look and feel of Minecraft".to_string(),
                    pack_format: MC_PACK_FORMAT,
                    icon: None,
                });
        }
    }

    /// Show the shader loading page and defer the synchronous GPU rebuild until
    /// the next redraw has submitted that page to the swapchain.
    pub(super) fn queue_shader_pack_load(
        &mut self,
        pack: String,
        overrides: std::collections::BTreeMap<String, String>,
        reason: ShaderPackLoadReason,
    ) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        renderer.screens.shader_pack_loading = true;
        renderer.screens.shader_pack_loading_name = pack.clone();
        renderer.screens.shader_pack_loading_stage =
            "Compiling shaders and building renderer pipelines".to_string();
        renderer.screens.shader_pack_status = format!("Loading: {pack}");
        renderer.invalidate_gui();
        self.shader_pack_load_frame_submitted = false;
        self.pending_shader_pack_load = Some(PendingShaderPackLoad {
            pack,
            overrides,
            reason,
        });
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    pub(super) fn scan_and_update_shader_packs(&mut self) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        let capabilities = crate::render::shader_pack::RenderCapabilities {
            ray_tracing: renderer.settings.ray_tracing_available,
            fsr3: renderer.settings.fsr3_available,
        };
        renderer.screens.shader_packs =
            crate::render::shader_pack::discover_shader_packs(capabilities);
        renderer.screens.selected_shader_pack = self.config.shader_pack.clone();
    }
}
