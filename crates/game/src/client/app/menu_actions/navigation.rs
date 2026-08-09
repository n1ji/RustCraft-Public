use crate::audio::AudioBackend;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use crate::ui::button_ids as btn;
use winit::event_loop::ActiveEventLoop;

use super::{
    adjusted_config_value, config_value_label, next_framerate, open_folder, previous_framerate,
    slider_to_framerate, slider_value_from_mouse,
};
use crate::client::app::tasks::{AuthTask, ConnectTask};
use crate::client::app::{App, PendingShaderPackLoad, ShaderPackLoadReason};
impl App {
    pub(crate) fn connect_to_configured_server(&mut self) {
        if self.connection.is_some() || self.connect_task.is_some() {
            return;
        }
        let addr = if matches!(self.state, GameState::Multiplayer) {
            self.servers
                .selected_address(self.selected_server)
                .unwrap_or(&self.server_address)
                .trim()
                .to_string()
        } else {
            self.server_address.trim().to_string()
        };
        if addr.is_empty() {
            return;
        }
        self.server_address = addr.clone();
        self.config.username = self.username.clone();
        self.config.save_default();
        self.session
            .push_system_line(self.ui.i18n.tf("rustcraft.connection.connecting", &[&addr]));
        self.connect_task = Some(ConnectTask::spawn(
            addr,
            self.username.clone(),
            self.account.clone(),
        ));
    }

    pub(crate) fn handle_button_click(&mut self, button_id: u32, event_loop: &ActiveEventLoop) {
        // MC 1.8: UI button click sound
        self.audio.play(crate::audio::SoundEvent {
            name: "random.click".to_string(),
            category: crate::audio::SoundCategory::Ui,
            volume: 1.0,
            pitch: 1.0,
            position: None,
        });
        if self.handle_slider_click(button_id) {
            return;
        }
        match button_id {
            btn::SINGLEPLAYER => {}
            btn::OPTIONS => self.open_options(),
            btn::QUIT => {
                // Drop renderer while the window is still alive so backend surface
                // cleanup doesn't access a destroyed window handle.
                self.renderer = None;
                self.connection = None;
                event_loop.exit();
            }
            btn::MULTIPLAYER => {
                self.state = GameState::Multiplayer;
                self.start_server_refresh();
            }
            btn::ALT_MANAGER => self.state = GameState::AltManager,
            btn::MODDING => self.open_modding(),
            btn::MODDING_BACK => self.return_to_previous_screen(),
            btn::MODDING_CONFIGURE => self.open_selected_mod_config(),
            btn::MOD_CONFIG_BACK => self.return_to_previous_screen(),
            btn::MOD_CONFIG_RESET_ALL => self.reset_all_mod_config(),
            btn::MODDING_RELOAD_ALL => {
                let report = self.scripts.reload_all();
                let loaded = report.loaded.len();
                let errors = report.errors.len();
                // Prune stale entries for mods that disappeared from disk.
                {
                    let known_ids: std::collections::HashSet<_> = self
                        .scripts
                        .loaded_mods()
                        .into_iter()
                        .map(|m| m.id)
                        .collect();
                    self.config
                        .mod_enabled
                        .retain(|id, _| known_ids.contains(id.as_str()));
                    self.config.save_default();
                }
                self.modding_status = if errors == 0 {
                    format!("Reloaded {loaded} mod(s) successfully")
                } else {
                    format!(
                        "Reloaded {loaded} mod(s); {errors} error(s): {}",
                        report.errors.join(" | ")
                    )
                };
            }
            btn::MODDING_TOGGLE => {
                let selected = self
                    .scripts
                    .loaded_mods()
                    .into_iter()
                    .nth(self.modding_selected);
                if let Some(mod_info) = selected {
                    let (verb, result) = if mod_info.enabled {
                        ("Disabled", self.scripts.disable(&mod_info.id))
                    } else {
                        ("Enabled", self.scripts.enable(&mod_info.id))
                    };
                    self.modding_status = match result {
                        Ok(()) => {
                            self.config
                                .mod_enabled
                                .insert(mod_info.id.clone(), !mod_info.enabled);
                            self.config.save_default();
                            format!("{verb} {}", mod_info.name)
                        }
                        Err(error) => format!("Could not change {}: {error}", mod_info.name),
                    };
                } else {
                    self.modding_status = "Select a mod first".to_string();
                }
            }
            btn::MODDING_RELOAD => {
                let selected = self
                    .scripts
                    .loaded_mods()
                    .into_iter()
                    .nth(self.modding_selected);
                if let Some(mod_info) = selected {
                    self.modding_status = match self.scripts.reload(&mod_info.id) {
                        Ok(()) => format!("Reloaded {}", mod_info.name),
                        Err(error) => format!("Could not reload {}: {error}", mod_info.name),
                    };
                } else {
                    self.modding_status = "Select a mod first".to_string();
                }
            }
            id if id >= btn::MODDING_ROW_BASE
                && id < btn::MODDING_ROW_BASE + btn::MODDING_ROW_MAX as u32 =>
            {
                let index = (id - btn::MODDING_ROW_BASE) as usize;
                if index < self.scripts.mod_count() {
                    self.modding_selected = index;
                }
            }
            id if id >= btn::MOD_CONFIG_PREVIOUS_BASE
                && id < btn::MOD_CONFIG_PREVIOUS_BASE + btn::MOD_CONFIG_ENTRY_MAX as u32 =>
            {
                let index = (id - btn::MOD_CONFIG_PREVIOUS_BASE) as usize;
                self.adjust_mod_config(index, -1);
            }
            id if id >= btn::MOD_CONFIG_NEXT_BASE
                && id < btn::MOD_CONFIG_NEXT_BASE + btn::MOD_CONFIG_ENTRY_MAX as u32 =>
            {
                let index = (id - btn::MOD_CONFIG_NEXT_BASE) as usize;
                self.adjust_mod_config(index, 1);
            }
            id if id >= btn::MOD_CONFIG_RESET_BASE
                && id < btn::MOD_CONFIG_RESET_BASE + btn::MOD_CONFIG_ENTRY_MAX as u32 =>
            {
                let index = (id - btn::MOD_CONFIG_RESET_BASE) as usize;
                self.reset_mod_config(index);
            }
            id if id >= btn::MOD_CONFIG_ROW_BASE
                && id < btn::MOD_CONFIG_ROW_BASE + btn::MOD_CONFIG_ENTRY_MAX as u32 =>
            {
                let index = (id - btn::MOD_CONFIG_ROW_BASE) as usize;
                if let Some(mod_id) = self.active_mod_config_id() {
                    if index
                        < self
                            .scripts
                            .config_entries(&mod_id)
                            .map_or(0, |rows| rows.len())
                    {
                        self.mod_config_selected = index;
                    }
                }
            }
            btn::ALT_LOGIN => {
                if self.auth_task.is_none() {
                    self.auth_status = "Opening Microsoft sign-in in your browser...".to_string();
                    self.auth_task = Some(AuthTask::login());
                }
            }
            btn::ALT_OFFLINE => {
                self.offline_username_input.clear();
                self.entering_offline_name = true;
            }
            btn::ALT_LOGOUT => {
                if let Some(uuid) = self
                    .accounts
                    .get(self.selected_account)
                    .and_then(|account| account.uuid.as_deref())
                {
                    let _ = crate::auth::cache::remove_account(uuid);
                }
                self.accounts = crate::auth::cache::load_accounts().unwrap_or_default();
                self.account_faces_dirty = true;
                self.selected_account = self
                    .selected_account
                    .min(self.accounts.len().saturating_sub(1));
                self.account = crate::auth::cache::load_account().ok().flatten();
                self.auth_status = "Account removed".to_string();
            }
            btn::ALT_USE => {
                if let Some(selected) = self.accounts.get(self.selected_account).cloned() {
                    if let Some(uuid) = selected.uuid.as_deref() {
                        let _ = crate::auth::cache::select_account(uuid);
                    }
                    self.username = selected
                        .username
                        .clone()
                        .unwrap_or_else(|| self.username.clone());
                    self.account = Some(selected);
                    self.update_local_skin();
                    self.config.username = self.username.clone();
                    self.config.save_default();
                    self.auth_status = "Selected account is now active".to_string();
                }
            }
            btn::ALT_BACK => self.state = GameState::MainMenu,
            btn::BACK_TO_GAME => self.capture_back_to_game(),
            btn::PAUSE_OPTIONS => self.open_options(),
            btn::DISCONNECT => self.disconnect_to_main_menu(),
            btn::RESPAWN => {
                crate::client::network::send_respawn_request(&self.connection);
            }
            btn::DEATH_TITLE_SCREEN => self.disconnect_to_main_menu(),
            btn::GUI_SCALE_DOWN => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.settings.gui_scale =
                        renderer.settings.gui_scale.saturating_sub(1).max(1);
                    self.config.gui_scale = renderer.settings.gui_scale;
                    self.config.save_default();
                }
            }
            btn::GUI_SCALE_UP => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.settings.gui_scale = (renderer.settings.gui_scale + 1).min(4);
                    self.config.gui_scale = renderer.settings.gui_scale;
                    self.config.save_default();
                }
            }
            btn::RENDER_DISTANCE_DOWN => {
                self.config.render_distance = self.config.render_distance.saturating_sub(2).max(2);
                self.save_and_sync_client_settings();
            }
            btn::RENDER_DISTANCE_UP => {
                self.config.render_distance = (self.config.render_distance + 2).min(16);
                self.save_and_sync_client_settings();
            }
            btn::SMOOTH_LIGHTING_TOGGLE => {
                self.config.smooth_lighting = !self.config.smooth_lighting;
                self.world.set_smooth_lighting(self.config.smooth_lighting);
                self.world.mesh_options.better_grass = self.config.better_grass;
                self.world.mesh_options.connected_textures = self.config.connected_textures;
                let meshes = self.world.build_all_meshes();
                if let Some(renderer) = &mut self.renderer {
                    renderer.upload_world(&meshes);
                }
                self.config.save_default();
            }
            btn::PARTICLES_TOGGLE => {
                self.config.particles = self.config.particles.next();
                self.config.save_default();
            }
            btn::MASTER_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Master, -0.1);
            }
            btn::MASTER_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Master, 0.1);
            }
            btn::MUSIC_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Music, -0.1);
            }
            btn::MUSIC_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Music, 0.1);
            }
            btn::BLOCKS_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Blocks, -0.1);
            }
            btn::BLOCKS_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Blocks, 0.1);
            }
            btn::HOSTILE_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Hostile, -0.1);
            }
            btn::HOSTILE_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Hostile, 0.1);
            }
            btn::FRIENDLY_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Friendly, -0.1);
            }
            btn::FRIENDLY_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Friendly, 0.1);
            }
            btn::WEATHER_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Weather, -0.1);
            }
            btn::WEATHER_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Weather, 0.1);
            }
            btn::AMBIENT_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Ambient, -0.1);
            }
            btn::AMBIENT_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Ambient, 0.1);
            }
            btn::PLAYERS_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Players, -0.1);
            }
            btn::PLAYERS_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Players, 0.1);
            }
            btn::UI_VOLUME_DOWN => {
                self.adjust_volume(crate::audio::SoundCategory::Ui, -0.1);
            }
            btn::UI_VOLUME_UP => {
                self.adjust_volume(crate::audio::SoundCategory::Ui, 0.1);
            }
            btn::AUDIO_DEVICE_CYCLE => {
                let devices = crate::audio::list_audio_devices();
                let current = &self.config.audio_device;
                let next_idx = devices
                    .iter()
                    .position(|d| d == current)
                    .map(|i| (i + 1) % devices.len())
                    .unwrap_or(0);
                self.config.audio_device = devices[next_idx].clone();
                self.config.save_default();
                self.audio.reinit(&self.config.audio_device);
            }
            btn::FOV_DOWN => {
                self.config.fov = (self.config.fov - 5.0).max(30.0);
                self.config.save_default();
            }
            btn::FOV_UP => {
                self.config.fov = (self.config.fov + 5.0).min(110.0);
                self.config.save_default();
            }
            btn::FRAMERATE_DOWN => {
                self.config.max_framerate = previous_framerate(self.config.max_framerate);
                self.config.save_default();
            }
            btn::FRAMERATE_UP => {
                self.config.max_framerate = next_framerate(self.config.max_framerate);
                self.config.save_default();
            }
            btn::FSR3_TOGGLE => {
                self.config.fsr3_enabled = !self.config.fsr3_enabled;
                self.config.save_default();
            }
            btn::FSR3_QUALITY_CYCLE => {
                self.config.fsr3_quality = self.config.fsr3_quality.next();
                self.config.save_default();
            }
            btn::FSR3_FRAME_GENERATION_TOGGLE => {
                self.config.fsr3_frame_generation = !self.config.fsr3_frame_generation;
                self.config.save_default();
            }
            btn::CLOUDS_TOGGLE => {
                self.config.clouds = !self.config.clouds;
                self.config.save_default();
            }
            btn::WEATHER_EFFECTS_TOGGLE => {
                self.config.weather_effects = !self.config.weather_effects;
                self.config.save_default();
            }
            btn::ENTITY_SHADOWS_TOGGLE => {
                self.config.entity_shadows = !self.config.entity_shadows;
                self.config.save_default();
            }
            btn::VIEW_BOBBING_TOGGLE => {
                self.config.view_bobbing = !self.config.view_bobbing;
                self.config.save_default();
            }
            btn::ADVANCED_TOOLTIPS_TOGGLE => {
                self.config.advanced_tooltips = !self.config.advanced_tooltips;
                self.config.save_default();
            }
            btn::BETTER_GRASS_TOGGLE => {
                self.config.better_grass = !self.config.better_grass;
                self.world.mesh_options.better_grass = self.config.better_grass;
                let meshes = self.world.build_all_meshes();
                if let Some(renderer) = &mut self.renderer {
                    renderer.upload_world(&meshes);
                }
                self.config.save_default();
            }
            btn::CONNECTED_TEXTURES_TOGGLE => {
                self.config.connected_textures = !self.config.connected_textures;
                self.world.mesh_options.connected_textures = self.config.connected_textures;
                let meshes = self.world.build_all_meshes();
                if let Some(renderer) = &mut self.renderer {
                    renderer.upload_world(&meshes);
                }
                self.config.save_default();
            }
            btn::LANGUAGE_TOGGLE => {
                self.config.language = if self.config.language == "zh_CN" {
                    "en_US".to_string()
                } else {
                    "zh_CN".to_string()
                };
                self.ui = crate::ui::UiState::new(&self.config.language);
                self.save_and_sync_client_settings();
            }
            btn::RESET_CONTROLS => {
                for action in crate::client::keybind::Action::all_bindable() {
                    match self.control_device {
                        crate::client::keybind::ControlDevice::KeyboardMouse => {
                            self.keybinds.reset_action(*action)
                        }
                        crate::client::keybind::ControlDevice::Gamepad => {
                            self.keybinds.reset_gamepad_action(*action)
                        }
                    }
                }
                self.config.keybinds = self.keybinds.clone();
                self.config.save_default();
                self.rebinding_action = None;
                self.session
                    .push_system_line(self.ui.t("controls.resetAll"));
            }
            btn::CONTROL_DEVICE_TOGGLE => {
                self.rebinding_action = None;
                self.control_device = match self.control_device {
                    crate::client::keybind::ControlDevice::KeyboardMouse => {
                        crate::client::keybind::ControlDevice::Gamepad
                    }
                    crate::client::keybind::ControlDevice::Gamepad => {
                        crate::client::keybind::ControlDevice::KeyboardMouse
                    }
                };
            }
            btn::INVERT_MOUSE_TOGGLE => {
                self.config.invert_mouse = !self.config.invert_mouse;
                self.config.save_default();
            }
            btn::SKIN_CUSTOMIZATION => self.push_subscreen(GameState::SkinCustomization {
                previous: Box::new(GameState::MainMenu),
            }),
            btn::SKIN_CAPE_TOGGLE => self.toggle_skin_part(0x01),
            btn::SKIN_JACKET_TOGGLE => self.toggle_skin_part(0x02),
            btn::SKIN_LEFT_SLEEVE_TOGGLE => self.toggle_skin_part(0x04),
            btn::SKIN_RIGHT_SLEEVE_TOGGLE => self.toggle_skin_part(0x08),
            btn::SKIN_LEFT_PANTS_TOGGLE => self.toggle_skin_part(0x10),
            btn::SKIN_RIGHT_PANTS_TOGGLE => self.toggle_skin_part(0x20),
            btn::SKIN_HAT_TOGGLE => self.toggle_skin_part(0x40),
            btn::SKIN_ALL_TOGGLE => {
                self.config.skin_parts = if self.config.skin_parts == 0x7f {
                    0
                } else {
                    0x7f
                };
                self.save_and_sync_client_settings();
            }
            btn::RESOURCE_PACK_ACCEPT => self.accept_resource_pack(),
            btn::RESOURCE_PACK_DECLINE => self.decline_resource_pack(),
            btn::RESOURCE_PACKS => {
                self.scan_and_update_resource_packs();
                self.push_subscreen(GameState::ResourcePacks {
                    previous: Box::new(GameState::MainMenu),
                })
            }
            btn::RESOURCE_PACK_OPEN_FOLDER => {
                let path = std::path::Path::new("resourcepacks");
                let _ = std::fs::create_dir_all(path);
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("explorer").arg(path).spawn();
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open").arg(path).spawn();
                }
                #[cfg(target_os = "linux")]
                {
                    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
                }
            }
            btn::SHADER_PACKS => {
                self.scan_and_update_shader_packs();
                self.push_subscreen(GameState::ShaderPacks {
                    previous: Box::new(GameState::MainMenu),
                });
            }
            btn::SHADER_PACK_OPEN_FOLDER => {
                open_folder(std::path::Path::new(
                    crate::render::shader_pack::SHADER_PACK_DIR,
                ));
                self.scan_and_update_shader_packs();
            }
            btn::SHADER_PACK_OFF => {
                self.pending_shader_pack_load = None;
                self.shader_pack_load_frame_submitted = false;
                self.config.shader_pack = None;
                self.config.save_default();
                self.scan_and_update_shader_packs();
                if let Some(renderer) = &mut self.renderer {
                    renderer.unload_optifine_shader_pack();
                    renderer.screens.shader_pack_status = "Shaders disabled".to_string();
                    renderer.screens.shader_pack_loading = false;
                    renderer.screens.shader_pack_loading_name.clear();
                    renderer.screens.shader_pack_loading_stage.clear();
                    renderer.invalidate_gui();
                }
            }
            btn::SHADER_OPTIONS => self.open_shader_options(),
            btn::SHADER_OPTIONS_BACK => self.shader_options_back(),
            btn::SHADER_OPTIONS_RESET_ALL => self.reset_all_shader_options(),
            id if id >= btn::SHADER_OPTION_ROW_BASE
                && id < btn::SHADER_OPTION_ROW_BASE + btn::SHADER_OPTION_ENTRY_MAX as u32 =>
            {
                self.shader_option_adjust((id - btn::SHADER_OPTION_ROW_BASE) as usize, 1, true);
            }
            id if id >= btn::SHADER_OPTION_PREVIOUS_BASE
                && id < btn::SHADER_OPTION_PREVIOUS_BASE + btn::SHADER_OPTION_ENTRY_MAX as u32 =>
            {
                self.shader_option_adjust(
                    (id - btn::SHADER_OPTION_PREVIOUS_BASE) as usize,
                    -1,
                    false,
                );
            }
            id if id >= btn::SHADER_OPTION_NEXT_BASE
                && id < btn::SHADER_OPTION_NEXT_BASE + btn::SHADER_OPTION_ENTRY_MAX as u32 =>
            {
                self.shader_option_adjust((id - btn::SHADER_OPTION_NEXT_BASE) as usize, 1, false);
            }
            id if id >= btn::SHADER_OPTION_RESET_BASE
                && id < btn::SHADER_OPTION_RESET_BASE + btn::SHADER_OPTION_ENTRY_MAX as u32 =>
            {
                self.reset_shader_option((id - btn::SHADER_OPTION_RESET_BASE) as usize);
            }
            btn::DONE => self.return_to_previous_screen(),
            btn::VIDEO_SETTINGS => self.push_subscreen(GameState::VideoSettings {
                previous: Box::new(GameState::MainMenu),
            }),
            btn::CONTROLS => self.push_subscreen(GameState::Controls {
                previous: Box::new(GameState::MainMenu),
            }),
            btn::LANGUAGE => self.push_subscreen(GameState::Language {
                previous: Box::new(GameState::MainMenu),
            }),
            btn::AUDIO_SETTINGS => self.push_subscreen(GameState::AudioSettings {
                previous: Box::new(GameState::MainMenu),
            }),
            btn::CHAT_SETTINGS => self.push_subscreen(GameState::ChatSettings {
                previous: Box::new(GameState::MainMenu),
            }),
            btn::CHAT_WIDTH_DOWN => {
                self.config.chat_width = (self.config.chat_width - 0.05).clamp(0.1, 1.0);
                self.sync_chat_settings();
                self.config.save_default();
            }
            btn::CHAT_WIDTH_UP => {
                self.config.chat_width = (self.config.chat_width + 0.05).clamp(0.1, 1.0);
                self.sync_chat_settings();
                self.config.save_default();
            }
            btn::CHAT_HEIGHT_DOWN => {
                self.config.chat_height = self.config.chat_height.saturating_sub(1).max(1);
                self.sync_chat_settings();
                self.config.save_default();
            }
            btn::CHAT_HEIGHT_UP => {
                self.config.chat_height = self.config.chat_height.saturating_add(1).min(30);
                self.sync_chat_settings();
                self.config.save_default();
            }
            btn::CHAT_BACKGROUND_TOGGLE => {
                self.config.chat_background = !self.config.chat_background;
                self.config.save_default();
            }
            btn::CHAT_OVERLAY_TOGGLE => {
                self.config.chat_overlay = !self.config.chat_overlay;
                self.config.save_default();
            }
            btn::CHAT_AVATARS_TOGGLE => {
                self.config.chat_player_avatars = !self.config.chat_player_avatars;
                self.config.save_default();
            }
            btn::TAB_AVATARS_TOGGLE => {
                self.config.tab_player_avatars = !self.config.tab_player_avatars;
                self.config.save_default();
            }
            btn::DIRECT_CONNECT => {
                self.state = GameState::DirectConnect;
                if let Some(window) = &self.window {
                    window.set_ime_allowed(true);
                }
            }
            btn::ADD_SERVER => {
                self.server_editor_name.clear();
                self.server_editor_address.clear();
                self.server_editor_address_focused = false;
                self.state = GameState::ServerEditor { edit_index: None };
                if let Some(window) = &self.window {
                    window.set_ime_allowed(true);
                }
            }
            btn::EDIT_SERVER => {
                if let Some(server) = self.servers.servers.get(self.selected_server).cloned() {
                    self.server_editor_name = server.name;
                    self.server_editor_address = server.address;
                    self.server_editor_address_focused = false;
                    self.state = GameState::ServerEditor {
                        edit_index: Some(self.selected_server),
                    };
                    if let Some(window) = &self.window {
                        window.set_ime_allowed(true);
                    }
                }
            }
            btn::SERVER_EDITOR_SAVE => {
                self.save_server_editor();
            }
            btn::SERVER_EDITOR_CANCEL => {
                self.close_server_editor();
            }
            btn::SERVER_NAME_FIELD => {
                self.server_editor_address_focused = false;
            }
            btn::SERVER_ADDRESS_FIELD => {
                self.server_editor_address_focused = true;
            }
            btn::DIRECT_ADDRESS_FIELD => {}
            btn::CONNECT => {
                self.connection = None;
                self.state = GameState::Connecting;
                self.connect_to_configured_server();
                self.mouse_captured = false;
                self.set_cursor_captured(false);
            }
            btn::REFRESH_SERVER_LIST => {
                self.start_server_refresh();
            }
            btn::DELETE_SERVER => {
                if self.servers.remove(self.selected_server) {
                    self.selected_server = self
                        .selected_server
                        .min(self.servers.servers.len().saturating_sub(1));
                }
            }
            btn::SAVE_DIRECT_SERVER => {
                let addr = self.server_address.trim().to_string();
                if !addr.is_empty() {
                    self.selected_server = self.servers.upsert(addr.clone(), addr);
                }
            }
            btn::MULTIPLAYER_BACK => {
                self.state = GameState::Multiplayer;
                if let Some(window) = &self.window {
                    window.set_ime_allowed(false);
                }
            }
            btn::MULTIPLAYER_CANCEL => {
                self.state = GameState::MainMenu;
                if let Some(window) = &self.window {
                    window.set_ime_allowed(false);
                }
            }
            id if id >= btn::SERVER_ROW_BASE
                && id < btn::SERVER_ROW_BASE + btn::SERVER_ROW_MAX as u32 =>
            {
                let idx = (id - btn::SERVER_ROW_BASE) as usize;
                if let Some(server) = self.servers.servers.get(idx).cloned() {
                    self.selected_server = idx;
                    self.server_address = server.address;
                    let now = std::time::Instant::now();
                    let double_clicked =
                        self.last_server_click
                            .is_some_and(|(last_idx, last_click)| {
                                last_idx == idx
                                    && now.duration_since(last_click)
                                        <= std::time::Duration::from_millis(250)
                            });
                    self.last_server_click = Some((idx, now));
                    if double_clicked {
                        self.connection = None;
                        self.state = GameState::Connecting;
                        self.connect_to_configured_server();
                        self.mouse_captured = false;
                        self.set_cursor_captured(false);
                    }
                }
            }
            id if id >= btn::ALT_ACCOUNT_ROW_BASE
                && id < btn::ALT_ACCOUNT_ROW_BASE + btn::ALT_ACCOUNT_ROW_MAX as u32 =>
            {
                let index = (id - btn::ALT_ACCOUNT_ROW_BASE) as usize;
                if index < self.accounts.len() {
                    self.selected_account = index;
                }
            }
            id if id >= btn::RESOURCE_PACK_BASE
                && id < btn::RESOURCE_PACK_BASE + btn::RESOURCE_PACK_MAX as u32 =>
            {
                let available_idx = (id - btn::RESOURCE_PACK_BASE) as usize;
                let name = self
                    .resource_pack_list
                    .iter()
                    .filter(|p| !p.enabled)
                    .nth(available_idx)
                    .map(|p| p.name.clone());
                if let Some(ref name) = name {
                    if !self.config.enabled_resource_packs.contains(name) {
                        self.config.enabled_resource_packs.insert(0, name.clone());
                    }
                }
                self.config.save_default();
                self.scan_and_update_resource_packs();
            }
            id if id >= btn::RESOURCE_PACK_SELECTED_BASE
                && id
                    < btn::RESOURCE_PACK_SELECTED_BASE + btn::RESOURCE_PACK_SELECTED_MAX as u32 =>
            {
                let selected_idx = (id - btn::RESOURCE_PACK_SELECTED_BASE) as usize;
                let name = self
                    .resource_pack_list
                    .iter()
                    .filter(|p| p.enabled)
                    .nth(selected_idx)
                    .map(|p| p.name.clone());
                if let Some(ref name) = name {
                    self.config
                        .enabled_resource_packs
                        .retain(|enabled| enabled != name);
                }
                self.config.save_default();
                self.scan_and_update_resource_packs();
            }
            id if id >= btn::RESOURCE_PACK_SELECTED_UP_BASE
                && id
                    < btn::RESOURCE_PACK_SELECTED_UP_BASE
                        + btn::RESOURCE_PACK_SELECTED_MAX as u32 =>
            {
                let index = (id - btn::RESOURCE_PACK_SELECTED_UP_BASE) as usize;
                if index > 0 && index < self.config.enabled_resource_packs.len() {
                    self.config.enabled_resource_packs.swap(index, index - 1);
                    self.config.save_default();
                    self.scan_and_update_resource_packs();
                }
            }
            id if id >= btn::RESOURCE_PACK_SELECTED_DOWN_BASE
                && id
                    < btn::RESOURCE_PACK_SELECTED_DOWN_BASE
                        + btn::RESOURCE_PACK_SELECTED_MAX as u32 =>
            {
                let index = (id - btn::RESOURCE_PACK_SELECTED_DOWN_BASE) as usize;
                if index + 1 < self.config.enabled_resource_packs.len() {
                    self.config.enabled_resource_packs.swap(index, index + 1);
                    self.config.save_default();
                    self.scan_and_update_resource_packs();
                }
            }
            id if id >= btn::SHADER_PACK_BASE
                && id < btn::SHADER_PACK_BASE + btn::SHADER_PACK_MAX as u32 =>
            {
                let index = (id - btn::SHADER_PACK_BASE) as usize;
                let selected = self.renderer.as_ref().and_then(|renderer| {
                    renderer
                        .screens
                        .shader_packs
                        .get(index)
                        .filter(|pack| pack.compatible)
                        .map(|pack| pack.source_name.clone())
                });
                if let Some(selected) = selected {
                    self.config.shader_pack = Some(selected.clone());
                    self.config.save_default();
                    self.scan_and_update_shader_packs();
                    // Stale sessions for another pack must not leak options.
                    self.shader_options = None;
                    let overrides = self
                        .config
                        .shader_pack_options
                        .get(&selected)
                        .cloned()
                        .unwrap_or_default();
                    self.queue_shader_pack_load(
                        selected,
                        overrides,
                        ShaderPackLoadReason::Selection,
                    );
                }
            }
            id if id >= btn::CONTROL_BIND_BASE
                && id < btn::CONTROL_BIND_BASE + btn::CONTROL_BIND_MAX as u32 =>
            {
                let idx = (id - btn::CONTROL_BIND_BASE) as usize;
                self.rebinding_action = crate::client::keybind::Action::from_bindable_index(idx);
            }
            id if id >= btn::CONTROL_RESET_BASE
                && id < btn::CONTROL_RESET_BASE + btn::CONTROL_BIND_MAX as u32 =>
            {
                let idx = (id - btn::CONTROL_RESET_BASE) as usize;
                if let Some(action) = crate::client::keybind::Action::from_bindable_index(idx) {
                    match self.control_device {
                        crate::client::keybind::ControlDevice::KeyboardMouse => {
                            self.keybinds.reset_action(action)
                        }
                        crate::client::keybind::ControlDevice::Gamepad => {
                            self.keybinds.reset_gamepad_action(action)
                        }
                    }
                    self.config.keybinds = self.keybinds.clone();
                    self.config.save_default();
                    if self.rebinding_action == Some(action) {
                        self.rebinding_action = None;
                    }
                }
            }
            _ => {}
        }
    }
}
