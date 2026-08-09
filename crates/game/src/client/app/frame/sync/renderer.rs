use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::frame::visual::{
    cape_rotation, entity_billboard, fishing_line_start, interpolated_chest_lid_angle,
    mod_config_row, player_skin_key, skull_skin_key,
};
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

impl App {
    pub(crate) fn sync_renderer_state(&mut self) {
        if self.last_ui_text_hash == 0 || self.session.locale != self.config.language {
            self.session.locale.clone_from(&self.config.language);
            self.session.text.inventory_rejected = self.ui.t("rustcraft.inventory.rejected");
            self.session.text.resource_pack_offered = self.ui.t("rustcraft.resourcePack.offered");
            self.session.text.opened_window = self.ui.t("rustcraft.inventory.openedWindow");
        }
        self.session.view_distance = self.config.render_distance;
        self.session.skin_parts = self.config.skin_parts;

        let target = if matches!(self.state, GameState::Playing)
            && self.mouse_captured
            && !self.inventory_open
            && !self.chat_open
        {
            client::interaction::target_block(&self.world, &self.player.camera, 4.5)
        } else {
            None
        };

        if self.renderer.is_none() {
            return;
        }

        let controls_open = matches!(&self.state, GameState::Controls { .. });
        let render_world_background = self.state.has_world_background();
        let alt_manager_open = matches!(&self.state, GameState::AltManager);
        let multiplayer_open = matches!(&self.state, GameState::Multiplayer);
        let server_editor_open = matches!(&self.state, GameState::ServerEditor { .. });
        let connection_screen_open = matches!(
            &self.state,
            GameState::Connecting | GameState::LoadingWorld | GameState::Disconnected { .. }
        );
        let server_address_open = matches!(
            &self.state,
            GameState::DirectConnect | GameState::Connecting | GameState::LoadingWorld
        );
        let modding_open = matches!(&self.state, GameState::Modding { .. });

        // Static world render metadata and downloaded skin completions only
        // change on the 20 Hz client tick. Re-hashing signs, skull NBT and the
        // entire player roster at render frequency wastes hundreds of passes
        // per second without making the result any fresher than vanilla.
        let static_render_tick = self.tick_timer.tick_count();
        let refresh_static_render_state = self.last_static_render_tick != static_render_tick;
        let (skull_hash, player_skin_update) =
            if render_world_background && refresh_static_render_state {
                self.last_static_render_tick = static_render_tick;
                let hash = self.compute_skull_hash();
                (Some(hash), self.collect_pending_player_skins(hash))
            } else {
                (None, None)
            };
        // Entity state is advanced by the fixed 20 Hz simulation.  Hashing
        // every tracked entity on every redraw made the render thread do an
        // O(entity_count) walk at 1000-FPS settings even though the result
        // could not change between ticks.  Keep the billboard generation
        // synchronized to the simulation tick; interpolation-sensitive data
        // (nametags and the first-person hand) is updated below without
        // rebuilding the complete entity batch.
        let entity_hash = if render_world_background {
            if refresh_static_render_state || self.last_entity_hash == 0 {
                self.compute_entity_hash()
            } else {
                self.last_entity_hash
            }
        } else {
            0
        };

        let active_config_id = match &self.state {
            GameState::ModConfig { mod_id, .. } => Some(mod_id.clone()),
            _ => None,
        };
        let mod_config_view = active_config_id.as_deref().map(|mod_id| {
            let mod_info = self
                .scripts
                .loaded_mods()
                .into_iter()
                .find(|info| info.id == mod_id);
            let title = mod_info
                .as_ref()
                .map(|info| format!("{} Configuration", info.name))
                .unwrap_or_else(|| format!("{mod_id} Configuration"));
            let locked = self.connection.is_some()
                && mod_info
                    .as_ref()
                    .is_some_and(|info| info.protocol_translator);
            let rows = self
                .scripts
                .config_entries(mod_id)
                .unwrap_or_default()
                .into_iter()
                .map(mod_config_row)
                .collect::<Vec<_>>();
            self.mod_config_selected = self.mod_config_selected.min(rows.len().saturating_sub(1));
            (title, rows, locked)
        });

        // The account screen owns the small player-face cache.  Keep this
        // data in the game/application layer so the screen renderer remains
        // RHI-only and never has to read authentication records or files.
        let account_faces = if alt_manager_open && self.account_faces_dirty {
            let faces = self.collect_account_faces();
            self.account_faces_dirty = false;
            Some(faces)
        } else {
            None
        };

        let Some(renderer) = &mut self.renderer else {
            return;
        };

        renderer.entities.entity_state_hash = entity_hash;
        let chest_alpha = self.tick_timer.alpha();
        if render_world_background
            && self.last_chest_structure_generation != self.world.chest_structure_generation
        {
            self.last_chest_structure_generation = self.world.chest_structure_generation;
            renderer.entities.chest_entries = self
                .world
                .chests
                .iter()
                .filter_map(|(&(x, y, z), _)| {
                    let block = self.world.get_block(x, y, z);
                    if !crate::world::is_chest_block(block) {
                        return None;
                    }
                    let same = |nx, ny, nz| self.world.get_block(nx, ny, nz) == block;
                    if block != crate::world::block::Block::EnderChest
                        && (same(x - 1, y, z) || same(x, y, z - 1))
                    {
                        return None;
                    }
                    let double_x =
                        block != crate::world::block::Block::EnderChest && same(x + 1, y, z);
                    let double_z =
                        block != crate::world::block::Block::EnderChest && same(x, y, z + 1);
                    let light = self.world.light_at_world(x, y, z);
                    Some(crate::render::scene::ChestRenderEntry {
                        position: [x, y, z],
                        block,
                        metadata: self.world.get_block_metadata(x, y, z),
                        lid_angle: 0.0,
                        double_x,
                        double_z,
                        sky_light: light.sky,
                        block_light: light.block,
                    })
                })
                .collect();
            renderer
                .entities
                .chest_entries
                .sort_unstable_by_key(|entry| entry.position);
        }
        if render_world_background {
            for entry in &mut renderer.entities.chest_entries {
                let position = (entry.position[0], entry.position[1], entry.position[2]);
                entry.lid_angle =
                    interpolated_chest_lid_angle(&self.world, position, entry.block, chest_alpha);
            }
            if refresh_static_render_state {
                for entry in &mut renderer.entities.skull_entries {
                    let light = self.world.light_at_world(
                        entry.position[0],
                        entry.position[1],
                        entry.position[2],
                    );
                    entry.sky_light = light.sky;
                    entry.block_light = light.block;
                }
            }
        }
        if let Some((pending, content_hash, layout_hash)) = player_skin_update {
            if renderer.entities.player_skin_content_hash != content_hash
                || renderer.entities.player_skin_layout_hash != layout_hash
            {
                renderer.entities.pending_player_skins = pending;
                renderer.entities.player_skin_content_hash = content_hash;
                renderer.entities.player_skin_layout_hash = layout_hash;
            }
        }

        for i in 0..9 {
            let slot = &self.inventory.slots[i];
            renderer.hud.hotbar_slots[i] = (slot.item_id, slot.count, slot.damage);
        }
        renderer.hud.hotbar_selected = self.inventory.selected;
        renderer.hud.inventory_open = self.inventory_open;
        if render_world_background && refresh_static_render_state {
            // Build from the current block state rather than hashing the raw
            // tile-entity map. Tile data can arrive before its chunk and stale
            // entries can survive an unload, so only real, currently loaded
            // sign blocks are allowed into render state.
            let mut sign_entries: Vec<_> = self
                .session
                .sign_data
                .iter()
                .filter_map(|(&pos, lines)| {
                    let block = self.world.get_block(pos.0, pos.1, pos.2);
                    let wall_mounted = block == crate::world::block::Block::WallSign;
                    if !wall_mounted && block != crate::world::block::Block::StandingSign {
                        return None;
                    }
                    Some(crate::render::scene::SignEntry {
                        position: [pos.0, pos.1, pos.2],
                        // Network decoding already converts the JSON component
                        // to plain text. Parsing it again loses nested content.
                        lines: lines.to_vec(),
                        wall_mounted,
                        metadata: self.world.get_block_metadata(pos.0, pos.1, pos.2),
                    })
                })
                .collect();
            sign_entries.sort_unstable_by_key(|entry| entry.position);

            let sign_hash = {
                use std::hash::Hasher;
                let mut h = fnv::FnvHasher::default();
                h.write_usize(sign_entries.len());
                for sign in &sign_entries {
                    for position in sign.position {
                        h.write_i32(position);
                    }
                    h.write_u8(sign.wall_mounted as u8);
                    h.write_u8(sign.metadata);
                    for line in &sign.lines {
                        h.write(line.as_bytes());
                        h.write_u8(0);
                    }
                }
                h.finish()
            };
            if sign_hash != self.last_sign_hash {
                self.last_sign_hash = sign_hash;
                renderer.hud.sign_entries.clone_from(&sign_entries);
                renderer.entities.sign_entries = sign_entries;
            }
        }
        if self.inventory_open {
            renderer.hud.inventory_window_id = self.inventory.open_window_id;
            renderer
                .hud
                .inventory_window_type
                .clone_from(&self.inventory.open_window_type);
            renderer
                .hud
                .inventory_window_title
                .clone_from(&self.inventory.open_window_title);
            renderer.hud.inventory_window_slot_count = self.inventory.open_window_slot_count;
            renderer.hud.inventory_window_slots.clear();
            renderer.hud.inventory_window_slots.extend(
                (0..self.inventory.open_window_slot_count)
                    .map(|slot| self.inventory.item_view_for_protocol_slot(slot as i16)),
            );
            renderer
                .hud
                .inventory_window_properties
                .clone_from(&self.inventory.open_window_properties);
        }
        let new_boxes = target
            .as_ref()
            .map(|target| target.boxes.clone())
            .unwrap_or_default();
        if new_boxes.len() != renderer.world.block_selection_boxes.len()
            || new_boxes
                .iter()
                .zip(renderer.world.block_selection_boxes.iter())
                .any(|(a, b)| a.min != b.min || a.max != b.max)
        {
            renderer.world.block_dirty = true;
        }
        renderer.world.block_selection_boxes = new_boxes;
        let new_dig_progress =
            if target.as_ref().map(|target| target.hit.pos) == self.dig.active_pos() {
                self.dig.progress()
            } else {
                0.0
            };
        if (new_dig_progress - renderer.world.dig_progress).abs() > 0.001 {
            renderer.world.block_dirty = true;
        }
        renderer.world.dig_progress = new_dig_progress;
        let new_dig_pos = self.dig.active_pos().map(|p| [p.0, p.1, p.2]);
        if new_dig_pos != renderer.world.dig_position {
            renderer.world.block_dirty = true;
        }
        renderer.world.dig_position = new_dig_pos;
        // Keep the migrated GUI/HUD renderer on the configured Minecraft GUI
        // scale. RenderSettings starts at zero, so omitting this sync silently
        // forced every screen and HUD element through `max(1)` at 1x.
        renderer.settings.gui_scale = self.config.gui_scale.clamp(1, 4);
        renderer.settings.render_distance = self.config.render_distance;
        renderer.settings.smooth_lighting = self.config.smooth_lighting;
        renderer.world.chunk_count_loaded = self.world.chunks.len();
        let sky_b =
            renderer_api::core::sky::SkyGradient::daylight_factor(self.session.day_time as f32);
        if (sky_b - renderer.world.sky_brightness_cached).abs() > 0.01 {
            renderer.world.sky_brightness_cached = sky_b;
            self.world.set_sky_brightness(sky_b);
        }
        let particles_label = self.config.particles.label();
        if renderer.settings.particles_label != particles_label {
            renderer.settings.particles_label.clear();
            renderer.settings.particles_label.push_str(particles_label);
        }
        renderer.settings.particles_enabled = self.config.particles.enabled();
        renderer.settings.master_volume = self.config.master_volume;
        renderer.settings.music_volume = self.config.music_volume;
        renderer.settings.blocks_volume = self.config.blocks_volume;
        renderer.settings.hostile_volume = self.config.hostile_volume;
        renderer.settings.friendly_volume = self.config.friendly_volume;
        renderer.settings.players_volume = self.config.players_volume;
        renderer.settings.ambient_volume = self.config.ambient_volume;
        renderer.settings.weather_volume = self.config.weather_volume;
        renderer.settings.ui_volume = self.config.ui_volume;
        let audio_device = self.audio.device_name();
        if renderer.settings.audio_device != audio_device {
            renderer.settings.audio_device.clear();
            renderer.settings.audio_device.push_str(audio_device);
        }
        renderer.settings.fov = self.config.fov;
        renderer.settings.max_framerate = self.config.max_framerate;
        renderer.settings.fsr3_enabled = self.config.fsr3_enabled;
        renderer.settings.fsr3_quality = self.config.fsr3_quality;
        renderer.settings.fsr3_sharpness = self.config.fsr3_sharpness.clamp(0.0, 1.0);
        renderer.settings.fsr3_frame_generation = self.config.fsr3_frame_generation;
        renderer.settings.clouds = self.config.clouds;
        renderer.settings.weather_effects = self.config.weather_effects;
        renderer.settings.entity_shadows = self.config.entity_shadows;
        renderer.settings.view_bobbing = self.config.view_bobbing;
        renderer.settings.advanced_tooltips = self.config.advanced_tooltips;
        renderer.settings.chat_width = self.config.chat_width.clamp(0.1, 1.0);
        renderer.settings.chat_height = self.config.chat_height.clamp(1, 30);
        renderer.settings.chat_background = self.config.chat_background;
        renderer.settings.chat_overlay = self.config.chat_overlay;
        renderer.settings.chat_player_avatars = self.config.chat_player_avatars;
        renderer.settings.tab_player_avatars = self.config.tab_player_avatars;
        renderer.hud.chat_width = renderer.settings.chat_width;
        renderer.hud.chat_height = renderer.settings.chat_height;
        renderer.hud.chat_background = renderer.settings.chat_background;
        renderer.hud.chat_overlay = renderer.settings.chat_overlay;
        renderer.hud.chat_player_avatars = renderer.settings.chat_player_avatars;
        renderer.hud.tab_player_avatars = renderer.settings.tab_player_avatars;
        renderer.settings.better_grass = self.config.better_grass;
        renderer.settings.connected_textures = self.config.connected_textures;
        // Difficulty is server/world state, not a client-configurable preference.
        renderer.settings.difficulty = self.session.difficulty;
        renderer.settings.skin_parts = self.config.skin_parts;
        let ui_text_hash = {
            use std::hash::Hasher;
            let mut h = fnv::FnvHasher::default();
            h.write(self.config.language.as_bytes());
            h.finish()
        };
        if ui_text_hash != self.last_ui_text_hash {
            self.last_ui_text_hash = ui_text_hash;
            renderer
                .settings
                .language_code
                .clone_from(&self.config.language);
            renderer.settings.language_name = self.ui.t("language.name");
            renderer.settings.ui_text = self.ui.text.clone();
        }
        if controls_open {
            renderer.settings.control_bindings = self.keybinds.control_rows_for_device(
                self.control_device,
                self.rebinding_action,
                &self.ui.i18n,
            );
            renderer.settings.rebinding_action = self.rebinding_action;
            renderer.settings.controls_gamepad = matches!(
                self.control_device,
                crate::client::keybind::ControlDevice::Gamepad
            );
        }
        renderer.settings.mouse_sensitivity = self.config.mouse_sensitivity;
        renderer.settings.invert_mouse = self.config.invert_mouse;
        renderer.settings.gamepad_look_sensitivity = self.config.gamepad_look_sensitivity;
        renderer.settings.gamepad_cursor_speed = self.config.gamepad_cursor_speed;
        if server_address_open {
            renderer
                .screens
                .server_address
                .clone_from(&self.server_address);
            renderer.screens.username.clone_from(&self.username);
        }
        if alt_manager_open {
            renderer.screens.account_name = self
                .account
                .as_ref()
                .and_then(|account| account.username.clone())
                .unwrap_or_else(|| "No Microsoft account selected".to_string());
            renderer.screens.account_status = if self.auth_task.is_some() {
                "Waiting for Microsoft sign-in...".to_string()
            } else {
                self.auth_status.clone()
            };
            renderer.screens.entering_offline_name = self.entering_offline_name;
            renderer
                .screens
                .offline_username_input
                .clone_from(&self.offline_username_input);
            renderer.screens.account_list = self
                .accounts
                .iter()
                .map(|account| {
                    let active = self
                        .account
                        .as_ref()
                        .is_some_and(|selected| selected.uuid == account.uuid);
                    (
                        account
                            .username
                            .clone()
                            .unwrap_or_else(|| "Unknown account".to_string()),
                        account
                            .uuid
                            .clone()
                            .or_else(|| account.username.clone())
                            .unwrap_or_default(),
                        active,
                    )
                })
                .collect();
            if let Some(faces) = account_faces {
                renderer.screens.account_faces = faces;
            }
            renderer.screens.selected_account = self.selected_account;
        }
        if connection_screen_open {
            renderer.screens.connection_status = match &self.state {
                GameState::Disconnected { reason } => reason.clone(),
                _ => {
                    if let Some(task) = &self.connect_task {
                        self.ui
                            .i18n
                            .tf("rustcraft.connection.connecting", &[&task.address])
                    } else if self.connection.is_some() {
                        if self
                            .connection
                            .as_ref()
                            .map(|connection| {
                                connection
                                    .connected
                                    .load(std::sync::atomic::Ordering::SeqCst)
                            })
                            .unwrap_or(false)
                        {
                            self.ui.t("rustcraft.connection.connected")
                        } else {
                            self.ui.t("rustcraft.connection.disconnected")
                        }
                    } else {
                        self.ui.t("rustcraft.connection.notConnected")
                    }
                }
            };
        }
        if multiplayer_open {
            let refreshing = self.server_refresh_task.is_some();
            if renderer.screens.server_refreshing != refreshing {
                renderer.screens.server_refreshing = refreshing;
                renderer.invalidate_gui();
            }
            // Copy the complete status record.  The old screen updated for
            // description/error/favicon changes too; comparing only ping and
            // player counts made a refresh appear to do nothing for offline
            // servers and servers whose icon/description changed.  Keep raw
            // favicon strings as a change detector so PNG decoding happens
            // only after a real status update, never once per frame.
            let status_changed = renderer.screens.server_list.len() != self.servers.servers.len()
                || self
                    .servers
                    .servers
                    .iter()
                    .enumerate()
                    .any(|(index, server)| {
                        let current = renderer.screens.server_list.get(index);
                        let favicon = renderer
                            .screens
                            .server_favicon_sources
                            .get(index)
                            .and_then(|source| source.as_deref());
                        current.is_none_or(|row| {
                            row.name != server.name
                                || row.address != server.address
                                || row.online != server.status.online
                                || row.ping_ms != server.status.ping_ms
                                || row.players_online != server.status.players_online
                                || row.players_max != server.status.players_max
                                || row.version_name != server.status.version_name
                                || row.description != server.status.description
                                || row.error != server.status.error
                                || favicon != server.status.favicon.as_deref()
                        })
                    });
            if status_changed {
                renderer.screens.server_list = self
                    .servers
                    .servers
                    .iter()
                    .map(|server| crate::render::scene::ServerListRow {
                        name: server.name.clone(),
                        address: server.address.clone(),
                        online: server.status.online,
                        ping_ms: server.status.ping_ms,
                        players_online: server.status.players_online,
                        players_max: server.status.players_max,
                        version_name: server.status.version_name.clone(),
                        description: server.status.description.clone(),
                        error: server.status.error.clone(),
                        favicon_pixels: server
                            .status
                            .favicon
                            .as_ref()
                            .and_then(|f| crate::client::server_list::decode_favicon(f)),
                    })
                    .collect();
                renderer.screens.server_favicon_sources = self
                    .servers
                    .servers
                    .iter()
                    .map(|server| server.status.favicon.clone())
                    .collect();
                renderer.invalidate_gui();
            }
            renderer.screens.selected_server = self.selected_server;
        }
        if server_editor_open {
            renderer
                .screens
                .server_editor_name
                .clone_from(&self.server_editor_name);
            renderer
                .screens
                .server_editor_address
                .clone_from(&self.server_editor_address);
            renderer.screens.server_editor_address_focused = self.server_editor_address_focused;
        }
        if modding_open {
            let modding_rows = self
                .scripts
                .loaded_mods()
                .into_iter()
                .map(|mod_info| crate::render::scene::ModManagerRow {
                    id: mod_info.id,
                    name: mod_info.name,
                    version: mod_info.version,
                    enabled: mod_info.enabled,
                    protocol_translator: mod_info.protocol_translator,
                    config_entries: mod_info.config_entries,
                    granted_permissions: mod_info.granted_permissions,
                    denied_permissions: mod_info.denied_permissions,
                })
                .collect::<Vec<_>>();
            self.modding_selected = self
                .modding_selected
                .min(modding_rows.len().saturating_sub(1));
            renderer.screens.modding_rows = modding_rows;
            renderer.screens.modding_selected = self.modding_selected;
            renderer
                .screens
                .modding_status
                .clone_from(&self.modding_status);
            renderer.screens.modding_connection_active = self.connection.is_some();
        }
        if let Some((title, rows, locked)) = mod_config_view {
            renderer.screens.mod_config_title = Some(title);
            renderer.screens.mod_config_rows = rows;
            renderer.screens.mod_config_selected = self.mod_config_selected;
            renderer.screens.mod_config_status = self.mod_config_status.clone();
            renderer.screens.mod_config_locked = locked;
        } else if renderer.screens.mod_config_title.take().is_some() {
            renderer.screens.mod_config_rows.clear();
            renderer.screens.mod_config_locked = false;
        }
        renderer.world.max_players = self.session.max_players;
        renderer.world.world_time = self.session.world_time;
        renderer.world.day_time = self.session.day_time;
        renderer.world.dimension = self.session.dimension;
        renderer.world.raining = self.session.game_state.raining;
        renderer.world.rain_level = self.session.game_state.rain_level;
        renderer.world.thunder_level = self.session.game_state.thunder_level;
        renderer.world.gamemode = self.session.gamemode;
        if renderer.world.level_type != self.session.level_type {
            renderer
                .world
                .level_type
                .clone_from(&self.session.level_type);
        }
        renderer.world.spawn_position = self.session.spawn_position;

        // Health animation
        let current_health = self.session.health;
        if (current_health - renderer.hud.health).abs() > 0.01 {
            renderer.hud.prev_health = renderer.hud.health;
            renderer.hud.health_timer = 20;
        }
        renderer.hud.health = current_health;

        // Food animation
        if renderer.hud.food != self.session.food {
            renderer.hud.prev_food = renderer.hud.food;
            renderer.hud.food_timer = 10;
        }

        // Armor from inventory
        renderer.hud.armor_points = crate::client::armor::total_armor_points(&self.inventory);

        // Absorption from potion effect
        renderer.hud.absorption = self
            .player
            .active_effects
            .iter()
            .find(|e| e.effect_id == 22)
            .map(|e| (e.amplifier as f32 + 1.0) * 4.0)
            .unwrap_or(0.0);

        renderer.hud.food = self.session.food;
        renderer.hud.saturation = self.session.saturation;

        // Decay animation timers
        if renderer.hud.health_timer > 0 {
            renderer.hud.health_timer -= 1;
        }
        if renderer.hud.food_timer > 0 {
            renderer.hud.food_timer -= 1;
        }
        renderer.hud.experience_bar = self.session.experience_bar;
        renderer.hud.experience_level = self.session.experience_level;
        renderer.hud.experience_total = self.session.experience_total;
        renderer.hud.chat_open = self.chat_open;
        if self.chat_open {
            renderer.hud.chat_input.clone_from(&self.chat_input);
        }
        let chat_content_changed = self.last_chat_generation != self.session.chat_generation
            || self.last_chat_locale != self.config.language;
        let mut chat_lines_changed = false;
        if chat_content_changed {
            self.last_chat_generation = self.session.chat_generation;
            self.last_chat_locale.clone_from(&self.config.language);
            let localized_chat_lines = self
                .session
                .chat_lines
                .iter()
                .zip(&self.session.chat_json)
                .map(|(text, json)| {
                    json.as_deref()
                        .and_then(|json| {
                            crate::client::session::localized_chat_text(json, &self.ui.i18n)
                        })
                        .unwrap_or_else(|| text.clone())
                })
                .collect::<Vec<_>>();
            chat_lines_changed = renderer.hud.chat_lines != localized_chat_lines;
            if chat_lines_changed {
                renderer.hud.chat_lines = localized_chat_lines;
            }
        }
        let chat_faces_changed = chat_content_changed
            || self.last_chat_skin_generation != self.last_skin_cache_generation
            || self.last_chat_profile_generation != self.session.player_profile_generation;
        if chat_faces_changed {
            self.last_chat_skin_generation = self.last_skin_cache_generation;
            self.last_chat_profile_generation = self.session.player_profile_generation;
            renderer.hud.chat_faces = self
                .session
                .chat_senders
                .iter()
                .map(|sender| {
                    sender.as_deref().and_then(|uuid| {
                        let player = self.session.player_list.get(uuid)?;
                        Some(self.skin_cache.face_for(
                            Some(uuid),
                            Some(&player.name),
                            player.skin_property.as_deref(),
                        ))
                    })
                })
                .collect();
        }
        if self.chat_open || chat_lines_changed {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            if self.chat_open {
                renderer.hud.chat_visible_time = now;
            }
            if chat_lines_changed {
                // Keep messages visible only when a new chat state arrives.
                renderer.hud.chat_last_message_time = now;
            }
        }
        let sign_editor_was_open = renderer.hud.sign_editor_open;
        renderer.hud.sign_editor_open = self.session.sign_editor.is_some();
        if let Some(editor) = &self.session.sign_editor {
            renderer.hud.sign_editor_lines = editor.lines.clone();
            renderer.hud.sign_editor_active_line = editor.active_line;
        } else if sign_editor_was_open {
            renderer.hud.sign_editor_lines = Default::default();
            renderer.hud.sign_editor_active_line = 0;
        }
        renderer.hud.book_editor_open = self.book_editor.is_some();
        if let Some(editor) = &self.book_editor {
            renderer.hud.book_pages.clone_from(&editor.pages);
            renderer.hud.book_page = editor.page;
            renderer.hud.book_signing = editor.signing;
            renderer.hud.book_title.clone_from(&editor.title);
        } else {
            renderer.hud.book_pages.clear();
            renderer.hud.book_page = 0;
            renderer.hud.book_signing = false;
            renderer.hud.book_title.clear();
        }
        renderer.hud.player_list_open = self.player_list_open;
        if self.player_list_open {
            renderer.hud.tab_header.clone_from(&self.session.tab_header);
            renderer.hud.tab_footer.clone_from(&self.session.tab_footer);
        }
        if renderer.hud.action_bar != self.session.action_bar {
            renderer.hud.action_bar.clone_from(&self.session.action_bar);
        }
        if renderer.hud.title_text != self.session.title.title {
            renderer
                .hud
                .title_text
                .clone_from(&self.session.title.title);
        }
        if renderer.hud.subtitle_text != self.session.title.subtitle {
            renderer
                .hud
                .subtitle_text
                .clone_from(&self.session.title.subtitle);
        }
        renderer.hud.title_alpha = self.session.title.alpha();
        if self.last_scoreboard_generation != self.session.scoreboard_generation {
            self.last_scoreboard_generation = self.session.scoreboard_generation;
            let (sidebar_title, sidebar_lines) = self.session.sidebar_lines();
            renderer.hud.sidebar_title = sidebar_title;
            renderer.hud.sidebar_lines = sidebar_lines
                .into_iter()
                .map(|line| crate::render::scene::SidebarLine {
                    display: line.display,
                    score: line.score,
                })
                .collect();
        }
        renderer.world.world_border_center = [
            self.session.world_border.center_x,
            self.session.world_border.center_z,
        ];
        renderer.world.world_border_diameter = self.session.world_border.diameter;
        renderer.world.world_border_warning_blocks = self.session.world_border.warning_blocks;
        if renderer.screens.server_brand != self.session.server_brand {
            renderer
                .screens
                .server_brand
                .clone_from(&self.session.server_brand);
        }
        renderer.screens.resource_pack_status = self
            .session
            .resource_pack
            .as_ref()
            .map(|pack| format!("{} {}", pack.status, pack.hash));
        let player_list_needs_update = self.last_player_list_generation
            != self.session.player_list_generation
            || self.last_player_list_avatar_setting != Some(self.config.tab_player_avatars)
            || (self.config.tab_player_avatars
                && self.last_player_list_skin_generation != self.last_skin_cache_generation);
        if player_list_needs_update {
            self.last_player_list_generation = self.session.player_list_generation;
            self.last_player_list_skin_generation = self.last_skin_cache_generation;
            self.last_player_list_avatar_setting = Some(self.config.tab_player_avatars);
            let mut rows: Vec<_> = self
                .session
                .player_list
                .values()
                .map(|player| {
                    (
                        self.session.player_display_name(player),
                        player.ping,
                        player.gamemode,
                        self.config.tab_player_avatars.then(|| {
                            self.skin_cache.face_for(
                                Some(&player.uuid),
                                Some(&player.name),
                                player.skin_property.as_deref(),
                            )
                        }),
                    )
                })
                .collect();
            rows.sort_by_cached_key(|entry| entry.0.to_lowercase());
            renderer.hud.player_list = rows
                .iter()
                .map(|(name, ping, gamemode, _)| (name.clone(), *ping, *gamemode))
                .collect();
            renderer.hud.player_list_faces = rows
                .into_iter()
                .filter_map(|(_, _, _, face)| face)
                .collect();
        }
        renderer.frame.entity_count = self.entities.count();
        let sound_event_count = self.audio.played_count();
        if renderer.frame.sound_event_count != sound_event_count {
            renderer.frame.sound_event_count = sound_event_count;
            renderer.frame.recent_sounds = self
                .audio
                .recent()
                .rev()
                .take(3)
                .map(|event| format!("{} {:.1}x {:.2}", event.name, event.volume, event.pitch))
                .collect();
        }

        // Only rebuild entity billboards when entity state actually changed.
        if render_world_background && entity_hash != self.last_entity_hash {
            self.last_entity_hash = entity_hash;
            renderer.entities.entity_billboard_generation = renderer
                .entities
                .entity_billboard_generation
                .wrapping_add(1);
            renderer.entities.entity_billboards = self
                .entities
                .entities
                .values()
                .map(|entity| {
                    entity_billboard(
                        entity,
                        &self.session,
                        &self.world,
                        self.player_skin_slims
                            .get(&entity.entity_id)
                            .copied()
                            .unwrap_or(false),
                        self.player_cape_ready.contains(&entity.entity_id),
                        fishing_line_start(
                            entity,
                            &self.entities,
                            &self.player,
                            self.session.entity_id,
                            renderer.hud.hand_swing_progress,
                        ),
                    )
                })
                .collect();
        }
        // RenderFish anchors the line to the angler's interpolated hand every
        // frame. The hook itself can be stationary while the player turns, so
        // this cannot be tied only to the entity-state generation above.
        let fishing_swing = renderer.hud.hand_swing_progress;
        let mut fishing_line_changed = false;
        for billboard in &mut renderer.entities.entity_billboards {
            let Some(entity) = self.entities.get(billboard.entity_id) else {
                continue;
            };
            let fishing_line = fishing_line_start(
                entity,
                &self.entities,
                &self.player,
                self.session.entity_id,
                fishing_swing,
            );
            fishing_line_changed |= billboard.fishing_line_start != fishing_line;
            billboard.fishing_line_start = fishing_line;
        }
        if fishing_line_changed {
            renderer.entities.entity_billboard_generation = renderer
                .entities
                .entity_billboard_generation
                .wrapping_add(1);
        }
        // Light changes when the world around an entity changes (torch placed,
        // block broken, etc.). Re-sampling every frame would invalidate the
        // cached entity mesh hashes and force a full rebuild of every entity,
        // starving the rest of the rendering pipeline and making scoreboard /
        // UI updates feel unresponsive. A once-per-tick update matches vanilla's
        // 20 Hz lightmap refresh and lets the mesh cache stay hit-warm.
        if refresh_static_render_state {
            let mut light_changed = false;
            for billboard in &mut renderer.entities.entity_billboards {
                let bx = billboard.position[0].floor() as i32;
                let eye_height = billboard.entity_type.eye_height()
                    - if billboard.entity_type == crate::entity::EntityType::Player
                        && billboard.sneaking
                    {
                        0.08
                    } else {
                        0.0
                    };
                let by = (billboard.position[1] + eye_height).floor() as i32;
                let bz = billboard.position[2].floor() as i32;
                let light = self.world.light_at_world(bx, by, bz);
                light_changed |=
                    billboard.sky_light != light.sky || billboard.block_light != light.block;
                billboard.sky_light = light.sky;
                billboard.block_light = light.block;
            }
            if light_changed {
                renderer.entities.entity_billboard_generation = renderer
                    .entities
                    .entity_billboard_generation
                    .wrapping_add(1);
            }
        }
        if let Some(skull_hash) = skull_hash {
            if skull_hash != self.last_skull_hash {
                self.last_skull_hash = skull_hash;
                renderer.entities.skull_entries = self
                    .world
                    .skulls
                    .iter()
                    .map(|(&(x, y, z), skull)| {
                        let light = self.world.light_at_world(x, y, z);
                        crate::render::scene::SkullRenderEntry {
                            position: [x, y, z],
                            block_metadata: self.world.get_block_metadata(x, y, z),
                            skull_type: skull.skull_type,
                            rotation: skull.rotation,
                            skin_key: skull_skin_key(skull),
                            sky_light: light.sky,
                            block_light: light.block,
                        }
                    })
                    .collect();
            }
        }
        if self.config.particles.enabled() {
            renderer.set_particles(self.particles.particles(), self.particles.generation());
        } else {
            renderer.set_particles(&[], self.particles.generation());
        }

        if self.player.camera_mode != 0 {
            let mut local_equipment: [Option<(u16, u16)>; 5] = Default::default();
            if let Some(slot) = self.inventory.slots.get(self.inventory.selected) {
                local_equipment[0] = Some((slot.item_id, slot.damage));
            }
            // S04 equipment slots are 1=boots, 2=leggings, 3=chest, 4=helmet.
            for (armor_index, slot) in self.inventory.armor.iter().enumerate() {
                if !slot.is_empty() {
                    local_equipment[4 - armor_index] = Some((slot.item_id, slot.damage));
                }
            }
            let local_eye_height = crate::entity::EntityType::Player.eye_height()
                - if self.player.sneaking { 0.08 } else { 0.0 };
            let local_light = self.world.light_at_world(
                self.player.render_position.x.floor() as i32,
                (self.player.render_position.y + local_eye_height).floor() as i32,
                self.player.render_position.z.floor() as i32,
            );
            let local_billboard = crate::render::scene::EntityBillboard {
                entity_id: i32::MIN,
                position: [
                    self.player.render_position.x,
                    self.player.render_position.y,
                    self.player.render_position.z,
                ],
                sky_light: local_light.sky,
                block_light: local_light.block,
                height: 1.8,
                width: 0.6,
                name: None,
                kind: crate::render::scene::EntityBillboardKind::Player,
                entity_type: crate::entity::EntityType::Player,
                health: Some((self.session.health, 20.0)),
                held_item: local_equipment[0].map(|e| e.0),
                equipment: local_equipment,
                item_id: None,
                item_damage: local_equipment[0].map(|e| e.1),
                item_nbt: None,
                swing_progress: renderer.hud.hand_swing_progress,
                skin_key: Some("player/local".to_string()),
                slim: self.local_skin.slim_arms || self.local_player_model.slim_arms,
                skin_parts_mask: self.session.skin_parts,
                has_cape: self.local_cape_pixels.is_some(),
                cape_rotation: cape_rotation(
                    self.player.render_chasing_position,
                    self.player.render_position,
                    self.player.body_yaw,
                    self.player.prev_distance_walked_modified,
                    self.player.distance_walked_modified,
                    self.player.prev_camera_yaw,
                    self.player.camera_yaw,
                    self.tick_timer.alpha(),
                    self.player.sneaking,
                ),
                yaw: self.player.body_yaw,
                pitch: self.player.camera.mc_pitch_degrees(),
                head_yaw: self.player.camera.mc_yaw_degrees(),
                limb_swing: self.player.limb_swing,
                limb_swing_amount: self.player.limb_swing_amount,
                sneaking: self.player.sneaking,
                blocking: self.item_use_active
                    && matches!(
                        local_equipment[0].map(|item| item.0),
                        Some(267 | 268 | 272 | 276 | 283)
                    ),
                invisible: self
                    .player
                    .active_effects
                    .iter()
                    .any(|effect| effect.effect_id == 14),
                riding: self.player.vehicle_id.is_some(),
                name_visible: false,
                age_ticks: self.tick_timer.tick_count() as f32,
                hover_start: 0.0,
                velocity: [0.0, 0.0, 0.0],
                hurt_alpha: 0.0,
                death_alpha: 0.0,
                swing_alpha: renderer.hud.hand_swing_progress,
                critical_alpha: 0.0,
                visual: crate::entity::EntityVisualState::default(),
                hanging: None,
                fishing_line_start: None,
                painting_lights: Vec::new(),
            };
            renderer
                .entities
                .entity_billboards
                .retain(|billboard| billboard.entity_id != i32::MIN);
            renderer
                .entities
                .entity_billboards
                .push(local_billboard.clone());
            renderer.entities.local_player_billboard = Some(local_billboard);
        } else {
            renderer
                .entities
                .entity_billboards
                .retain(|billboard| billboard.entity_id != i32::MIN);
            renderer.entities.local_player_billboard = None;
        }

        renderer.settings.camera_mode = self.player.camera_mode;
        renderer.hud.first_person_arm_yaw = self.player.render_arm_yaw;
        renderer.hud.first_person_arm_pitch = self.player.render_arm_pitch;
        renderer.hud.first_person_prev_arm_yaw = self.player.prev_render_arm_yaw;
        renderer.hud.first_person_prev_arm_pitch = self.player.prev_render_arm_pitch;
        renderer.entities.local_model_parts = self.local_player_model.parts.len();
        let (skin_w, skin_h) = self.local_skin.dimensions();
        renderer.entities.local_skin_size = [skin_w, skin_h];
        let new_slim = self.local_skin.slim_arms || self.local_player_model.slim_arms;
        if self.local_skin_dirty {
            self.local_skin_dirty = false;
            renderer.entities.local_skin_slim = new_slim;
            renderer.entities.local_skin_face = self.local_skin.face_pixels();
            renderer.entities.local_skin_preview = self.local_skin.preview_pixels();
            renderer.entities.local_skin = self.local_skin.clone();
            renderer.update_skin_gpu();
            if let Some(ref pixels) = self.local_cape_pixels {
                renderer.upload_cape_to_atlas(pixels);
            }
        }

        // First-person hand state
        let selected_item = self.inventory.slots[self.inventory.selected];

        // Trigger once on the attack edge, then re-trigger during continuous mining
        if self.attack_held && !self.prev_attack_held {
            renderer.trigger_hand_swing();
        }
        self.prev_attack_held = self.attack_held;

        renderer.update_hand_state(
            selected_item.item_id,
            // Bow icon variants are keyed by damage 1-3 (pulling frames), so
            // durability wear must not leak into the held-item texture.
            if selected_item.item_id == 261 {
                0
            } else {
                selected_item.damage
            },
            self.inventory.slot_meta[self.inventory.selected]
                .nbt
                .clone(),
        );
        // Continuous mining swing: re-trigger when previous swing completes
        if self.attack_held
            && renderer.hud.hand_swing_timer <= 0.0
            && (self.dig.active_pos().is_some()
                || (self.item_use_active
                    && crate::client::app::block_interaction::is_sword(selected_item.item_id)))
        {
            renderer.trigger_hand_swing();
        }
        let held_id = selected_item.item_id;
        let previous_hand_use_kind = renderer.hud.hand_use_kind;
        renderer.hud.hand_use_kind = if self.item_use_active {
            match held_id {
                267 | 268 | 272 | 276 | 283 => 1, // swords: BLOCK
                373 => 3,                         // potion: DRINK
                261 => 4,                         // bow
                _ => 2,                           // food: EAT
            }
        } else {
            0
        };

        if held_id == 261 && self.item_use_active {
            let pull = (self.item_use_timer / 1.0).min(1.0);
            let bow_damage = if pull >= 0.9 {
                3
            } else if pull >= 0.65 {
                2
            } else {
                1
            };
            renderer.hud.hand_item_damage = bow_damage;
        } else if held_id == 261 {
            // Not drawing: standby icon. Durability wear must not select a
            // pulling frame (those are keyed by damage 1-3).
            renderer.hud.hand_item_damage = 0;
        }
        let alpha = self.tick_timer.alpha();
        renderer.hud.hand_use_progress =
            self.prev_item_use_timer + (self.item_use_timer - self.prev_item_use_timer) * alpha;
        renderer
            .hud
            .active_potion_effects
            .clone_from(&self.player.active_effects);
        renderer.hud.time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f32();
        let use_kind = renderer.hud.hand_use_kind;
        let now = std::time::Instant::now();
        const SCRIPT_ANIMATION_INTERVAL: std::time::Duration =
            std::time::Duration::from_micros(8_333); // 120 Hz
                                                     // A use-action transition must dispatch immediately. Otherwise the
                                                     // render frame that enters/leaves BLOCK can bake a mesh with the
                                                     // previous script matrix and vanilla flags, then visibly snap once the
                                                     // 120 Hz script throttle catches up.
        if renderer.hud.hand_use_kind != previous_hand_use_kind
            || now.duration_since(self.last_script_frame) >= SCRIPT_ANIMATION_INTERVAL
        {
            self.last_script_frame = now;

            // Lua animation callbacks are decoupled from the uncapped world
            // renderer. 120 Hz preserves smooth partial-tick hand animation,
            // while cached matrices can be reused by intervening GPU frames.
            let animation_context = crate::render::first_person::FirstPersonAnimationContext {
                hand: crate::render::first_person::Hand::MainHand,
                item_id: crate::render::first_person::item_resource_id(
                    held_id,
                    selected_item.damage,
                ),
                numeric_item_id: held_id,
                item_type: crate::render::first_person::ItemType::classify(held_id, use_kind),
                use_action: crate::render::first_person::UseAction::from_use_kind(use_kind),
                equip_progress: 1.0 - renderer.hud.hand_equip_progress,
                previous_equip_progress: 1.0 - renderer.hud.hand_equip_progress,
                swing_progress: renderer.hud.hand_swing_progress,
                previous_swing_progress: renderer.hud.hand_swing_progress,
                swinging: renderer.hud.hand_swing_timer > 0.0
                    || renderer.hud.hand_swing_progress > 0.0,
                swing_duration_ticks: 6,
                use_progress: renderer.hud.hand_use_progress,
                use_ticks: (self.item_use_timer.max(0.0) * 20.0).floor() as u32,
                remaining_use_ticks: self.item_use_timer.max(0.0).floor() as u32,
                max_use_ticks: match use_kind {
                    1 => 0,  // block is indefinite
                    2 => 32, // eat ~32 ticks (1.6s)
                    3 => 32, // drink ~32 ticks
                    4 => 72, // bow charge
                    _ => 0,
                },
                attack_cooldown: 1.0,
                using_item: use_kind != 0,
                blocking: use_kind == 1,
                attack_pressed: self.attack_held && !self.prev_attack_held,
                attack_held: self.attack_held,
                use_pressed: self.use_held,
                use_held: self.use_held,
                sneaking: self.player.sneaking,
                yaw: self.player.camera.mc_yaw_degrees(),
                pitch: self.player.camera.mc_pitch_degrees(),
                partial_tick: alpha,
                fov: self.player.camera.fov,
                aspect_ratio: self.player.camera.aspect,
            };
            let animation_transforms = self.scripts.dispatch_first_person(&animation_context);
            renderer.hud.first_person_arm_transform = animation_transforms.combined_arm();
            renderer.hud.first_person_item_transform = animation_transforms.combined_item();
            renderer.hud.fp_vanilla_flags = animation_transforms.vanilla_flags.clone();
        }

        const SCRIPT_HUD_INTERVAL: std::time::Duration = std::time::Duration::from_micros(16_667); // 60 Hz
        if !self.hud_hidden && now.duration_since(self.last_script_hud_frame) >= SCRIPT_HUD_INTERVAL
        {
            let script_delta = now
                .duration_since(self.last_script_hud_frame)
                .as_secs_f32()
                .min(0.25);
            self.last_script_hud_frame = now;

            // HUD input indicators remain responsive at display refresh speed,
            // while expensive Lua text/layout work is not repeated at 1000 Hz.
            let viewport = self
                .window
                .as_ref()
                .map(|window| window.inner_size())
                .unwrap_or(winit::dpi::PhysicalSize::new(1280, 720));
            let script_frame = crate::render::hooks::ScriptFrameContext {
                delta_time: script_delta,
                viewport_width: viewport.width,
                viewport_height: viewport.height,
            };
            renderer.hud.script_hud_before_commands = self
                .scripts
                .dispatch_render("render.hud.before", script_frame);
            let remaining_commands =
                4096usize.saturating_sub(renderer.hud.script_hud_before_commands.len());
            renderer.hud.script_hud_commands = self
                .scripts
                .dispatch_render("render.hud.after", script_frame);
            renderer
                .hud
                .script_hud_commands
                .truncate(remaining_commands);
        } else if self.hud_hidden
            && (!renderer.hud.script_hud_before_commands.is_empty()
                || !renderer.hud.script_hud_commands.is_empty())
        {
            // Hiding the vanilla HUD must also hide mod-rendered HUD commands,
            // and avoids retaining a stale draw list until F1 is pressed again.
            renderer.hud.script_hud_before_commands.clear();
            renderer.hud.script_hud_commands.clear();
        }

        if self.inventory_open {
            for i in 0..36 {
                renderer.hud.inventory_slots[i] =
                    self.inventory.slots[i].view_with_meta(Some(&self.inventory.slot_meta[i]));
            }
            for i in 0..4 {
                renderer.hud.inventory_armor_slots[i] =
                    self.inventory.armor[i].view_with_meta(Some(&self.inventory.armor_meta[i]));
            }
            for i in 0..5 {
                renderer.hud.inventory_crafting_slots[i] = self.inventory.crafting[i]
                    .view_with_meta(Some(&self.inventory.crafting_meta[i]));
            }
            renderer.hud.inventory_cursor_slot = self
                .inventory
                .cursor
                .view_with_meta(Some(&self.inventory.cursor_meta));
        }
    }
}
