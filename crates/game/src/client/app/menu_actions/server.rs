use crate::audio::AudioBackend;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use crate::ui::button_ids as btn;
use winit::event_loop::ActiveEventLoop;

use super::{slider_to_framerate, slider_value_from_mouse};
use crate::client::app::tasks::ServerRefreshTask;
use crate::client::app::{App, PendingShaderPackLoad, ShaderPackLoadReason};

impl App {
    pub(crate) fn save_server_editor(&mut self) -> bool {
        let name = self.server_editor_name.trim().to_string();
        let address = self.server_editor_address.trim().to_string();
        if name.is_empty() || address.is_empty() {
            return false;
        }

        let edit_index = match &self.state {
            GameState::ServerEditor { edit_index } => *edit_index,
            _ => return false,
        };
        self.selected_server = if let Some(index) = edit_index {
            if !self.servers.update(index, name, address.clone()) {
                return false;
            }
            index
        } else {
            self.servers.add(name, address.clone())
        };
        self.server_address = address;
        self.close_server_editor();
        true
    }

    pub(crate) fn close_server_editor(&mut self) {
        self.state = GameState::Multiplayer;
        self.last_server_click = None;
        if let Some(window) = &self.window {
            window.set_ime_allowed(false);
        }
    }

    pub(crate) fn handle_slider_click(&mut self, button_id: u32) -> bool {
        let hit = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.gui_hit_rect(button_id))
            .copied();
        let Some(hit) = hit else {
            return false;
        };
        let value = slider_value_from_mouse(self.mouse_x as f32, hit.x, hit.w, hit.h);

        if self.set_slider_value(button_id, value) {
            // Vanilla GuiOptionSlider starts dragging immediately on mouse press.
            self.active_slider = Some(button_id);
            true
        } else {
            false
        }
    }

    pub(crate) fn update_active_slider(&mut self) {
        let Some(button_id) = self.active_slider else {
            return;
        };
        let hit = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.gui_hit_rect(button_id))
            .copied();
        let Some(hit) = hit else {
            self.active_slider = None;
            return;
        };
        let value = slider_value_from_mouse(self.mouse_x as f32, hit.x, hit.w, hit.h);
        self.set_slider_value(button_id, value);
    }

    pub(crate) fn set_slider_value(&mut self, button_id: u32, value: f32) -> bool {
        match button_id {
            btn::GUI_SCALE_DOWN => {
                self.config.gui_scale = (1 + (value * 3.0).round() as u32).clamp(1, 4);
                if let Some(renderer) = &mut self.renderer {
                    renderer.settings.gui_scale = self.config.gui_scale;
                }
                self.config.save_default();
                true
            }
            btn::RENDER_DISTANCE_DOWN => {
                self.config.render_distance = 2 + (value * 7.0).round() as u8 * 2;
                self.save_and_sync_client_settings();
                true
            }
            btn::FOV_DOWN => {
                self.config.fov = 30.0 + value * 80.0;
                self.config.save_default();
                true
            }
            btn::FRAMERATE_DOWN => {
                self.config.max_framerate = slider_to_framerate(value);
                self.config.save_default();
                true
            }
            btn::FSR3_SHARPNESS => {
                self.config.fsr3_sharpness = value.clamp(0.0, 1.0);
                self.config.save_default();
                true
            }
            btn::MASTER_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Master, value);
                true
            }
            btn::MUSIC_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Music, value);
                true
            }
            btn::BLOCKS_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Blocks, value);
                true
            }
            btn::HOSTILE_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Hostile, value);
                true
            }
            btn::FRIENDLY_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Friendly, value);
                true
            }
            btn::WEATHER_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Weather, value);
                true
            }
            btn::AMBIENT_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Ambient, value);
                true
            }
            btn::PLAYERS_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Players, value);
                true
            }
            btn::UI_VOLUME_DOWN => {
                self.set_volume_absolute(crate::audio::SoundCategory::Ui, value);
                true
            }
            btn::MOUSE_SENSITIVITY => {
                self.config.mouse_sensitivity = value;
                self.config.save_default();
                true
            }
            btn::GAMEPAD_LOOK_SENSITIVITY => {
                self.config.gamepad_look_sensitivity = value;
                self.config.save_default();
                true
            }
            btn::GAMEPAD_CURSOR_SPEED => {
                self.config.gamepad_cursor_speed = value;
                self.config.save_default();
                true
            }
            btn::CHAT_WIDTH_DOWN => {
                self.config.chat_width = (0.1 + value * 0.9).clamp(0.1, 1.0);
                self.sync_chat_settings();
                self.config.save_default();
                true
            }
            btn::CHAT_HEIGHT_DOWN => {
                self.config.chat_height = ((1.0 + value * 29.0).round() as u8).clamp(1, 30);
                self.sync_chat_settings();
                self.config.save_default();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn start_server_refresh(&mut self) {
        if self.server_refresh_task.is_some() {
            return;
        }
        let servers = crate::client::server_list::ServerList::load_default();
        self.servers = servers.clone();
        self.selected_server = self
            .selected_server
            .min(self.servers.servers.len().saturating_sub(1));
        self.server_refresh_task = Some(ServerRefreshTask::spawn(servers));
        if let Some(renderer) = &mut self.renderer {
            renderer.invalidate_gui();
        }
    }

    pub(crate) fn return_to_previous_screen(&mut self) {
        let was_rp = matches!(self.state, GameState::ResourcePacks { .. });
        match std::mem::replace(&mut self.state, GameState::MainMenu) {
            GameState::Options { previous }
            | GameState::VideoSettings { previous }
            | GameState::SkinCustomization { previous }
            | GameState::Language { previous }
            | GameState::AudioSettings { previous }
            | GameState::ChatSettings { previous }
            | GameState::ResourcePacks { previous }
            | GameState::ShaderPacks { previous } => {
                self.state = *previous;
                if was_rp {
                    self.scan_and_update_resource_packs();
                    self.reload_assets();
                }
            }
            GameState::Modding { previous } | GameState::ModConfig { previous, .. } => {
                self.state = *previous
            }
            GameState::ShaderOptions { previous } => {
                self.state = *previous;
                self.apply_shader_options_if_dirty();
            }
            GameState::Controls { previous } => {
                self.rebinding_action = None;
                self.state = *previous;
            }
            _ => {}
        }
    }

    pub(crate) fn enter_playing(&mut self, connect: bool) {
        self.state = GameState::Playing;
        if connect {
            self.connect_to_configured_server();
        }
        self.world.set_smooth_lighting(self.config.smooth_lighting);
        let meshes = self.world.build_all_meshes();
        if let Some(renderer) = &mut self.renderer {
            renderer.upload_world(&meshes);
        }
        self.set_cursor_captured(true);
        self.mouse_captured = true;
        self.tick_timer = TickTimer::new();
    }

    pub(crate) fn capture_back_to_game(&mut self) {
        if matches!(self.state, GameState::Disconnected { .. }) {
            self.disconnect_to_main_menu();
        } else {
            self.state = GameState::Playing;
            self.mouse_captured = true;
            self.set_cursor_captured(true);
        }
    }

    pub(crate) fn disconnect_world(&mut self, reason: &str) {
        self.attack_held = false;
        self.use_held = false;
        self.use_presses_pending = 0;
        self.pending_attacks = 0;
        self.dig.cancel();
        self.pending_dig_cancel = None;
        self.block_hit_delay = 0;
        self.mouse_captured = false;
        self.set_cursor_captured(false);
        self.inventory_open = false;
        self.inventory_action_number = 0;
        self.unload_server_state();
        self.connection = None;
        self.scripts.notify_disconnect(reason);
        self.state = GameState::Disconnected {
            reason: reason.to_string(),
        };
    }

    pub(crate) fn disconnect_to_main_menu(&mut self) {
        self.state = GameState::MainMenu;
        self.attack_held = false;
        self.use_held = false;
        self.use_presses_pending = 0;
        self.pending_attacks = 0;
        self.dig.cancel();
        self.pending_dig_cancel = None;
        self.block_hit_delay = 0;
        self.inventory_open = false;
        self.inventory_action_number = 0;
        self.connection = None;
        self.scripts.set_connection_active(false);
        self.mouse_captured = false;
        self.set_cursor_captured(false);
        self.unload_server_state();
    }

    /// Drop every piece of state owned by the current server connection.
    ///
    /// This mirrors the client-side part of vanilla `Minecraft.loadWorld(null)`: the
    /// world and its renderer, entities, player/session data, transient effects, and
    /// sounds must all stop together.  Leaving the empty chunk meshes unapplied kept
    /// the old world allocated in the Vulkan renderer even after `World` was cleared.
    pub(crate) fn unload_server_state(&mut self) {
        self.scripts.clear_world_snapshot();
        self.last_script_world_block_key = None;
        let removed_meshes = self.world.clear_server_world();
        if let Some(renderer) = &mut self.renderer {
            renderer.upload_world_partial(&removed_meshes);
        }

        self.entities = crate::entity::EntityManager::new();
        self.network_state = crate::client::network::ClientNetworkState::new();
        self.inventory = crate::client::inventory::Inventory::new();
        self.inventory_open = false;
        self.particles = crate::client::particles::ParticleSystem::new(4096);
        self.audio.stop_all();

        // Keep only client preferences; all server-provided session state (including
        // scoreboard, tab list, titles, chat, resource-pack offer, and world border)
        // belongs to the world that was just unloaded.
        self.session = crate::client::session::SessionState::new();
        self.session.locale = self.config.language.clone();
        self.session.view_distance = self.config.render_distance;
        self.session.skin_parts = self.config.skin_parts;

        // Vanilla discards its local player when loading a null world. Preserve only
        // the viewport aspect ratio needed by the menu renderer.
        let aspect = self.player.camera.aspect;
        self.player =
            crate::client::player::Player::new(nalgebra::Point3::new(0.0, 64.0, 0.0), aspect);

        self.last_entity_hash = 0;
        self.last_ui_text_hash = 0;
        self.last_sign_hash = 0;
        self.last_skull_hash = 0;
        self.last_player_list_generation = u64::MAX;
        self.last_scoreboard_generation = u64::MAX;
        self.last_player_skin_roster_hash = u64::MAX;
        self.last_player_skin_generation = u64::MAX;
        self.last_player_skin_skull_hash = u64::MAX;
        self.last_skin_cache_generation = u64::MAX;
        self.player_skin_slims.clear();
        self.player_skin_profiles.clear();
    }
}
