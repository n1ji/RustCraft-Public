use std::collections::BTreeMap;

use crate::assets;
use crate::assets::resource_pack::ResourcePack;
use crate::audio;
use crate::client;
use crate::client::gamepad::GamepadInput;
use crate::client::interaction::DigController;
use crate::client::inventory::Inventory;
use crate::client::keybind::Action;
use crate::client::keybind::{ControlDevice, InputState, KeyBindings};
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use crate::net;
use crate::ui;
use crate::world;

use winit::window::Window;

use super::assets::{
    apply_audio_config, load_local_cape_pixels, load_local_skin, scan_resource_packs,
    ResourcePackInfo,
};
use super::{menu_actions, script_bridge, tasks};

#[derive(Clone, Copy, Default)]
pub(crate) struct AppFrameProfile {
    pub(crate) total_us: u64,
    pub(crate) tasks_us: u64,
    pub(crate) network_us: u64,
    pub(crate) world_us: u64,
    pub(crate) tick_us: u64,
    pub(crate) sync_us: u64,
    pub(crate) render_us: u64,
    pub(crate) other_us: u64,
    pub(crate) script_us: u64,
    pub(crate) script_callbacks: u32,
    pub(crate) script_slow_callbacks: u32,
    pub(crate) network_debug: crate::client::network::NetworkDebugProfile,
}

#[derive(Clone, Copy)]
pub(crate) enum ShaderPackLoadReason {
    Selection,
    OptionsReload,
}

pub(crate) struct PendingShaderPackLoad {
    pub(crate) pack: String,
    pub(crate) overrides: BTreeMap<String, String>,
    pub(crate) reason: ShaderPackLoadReason,
}

pub(crate) struct PlayerSkinProfileCache {
    pub(crate) identity: String,
    pub(crate) skin_property: Option<String>,
}

impl PlayerSkinProfileCache {
    pub(crate) fn new(
        identity: String,
        listed_property: Option<Option<&str>>,
        entity_property: Option<&str>,
    ) -> Self {
        Self {
            identity,
            skin_property: listed_property
                .flatten()
                .or(entity_property)
                .map(str::to_owned),
        }
    }

    pub(crate) fn update(
        &mut self,
        identity: String,
        listed_property: Option<Option<&str>>,
        entity_property: Option<&str>,
    ) {
        if self.identity != identity {
            self.identity = identity;
            self.skin_property = listed_property
                .flatten()
                .or(entity_property)
                .map(str::to_owned);
        } else if let Some(property) = listed_property {
            self.skin_property = property.or(entity_property).map(str::to_owned);
        } else if self.skin_property.is_none() {
            self.skin_property = entity_property.map(str::to_owned);
        }
    }
}

#[cfg(test)]
mod player_skin_profile_tests {
    use super::PlayerSkinProfileCache;

    #[test]
    fn tab_removal_keeps_profile_until_entity_identity_changes() {
        let mut profile =
            PlayerSkinProfileCache::new("uuid:one".to_string(), Some(Some("texture")), None);

        profile.update("uuid:one".to_string(), None, None);
        assert_eq!(profile.skin_property.as_deref(), Some("texture"));

        profile.update("uuid:one".to_string(), Some(None), None);
        assert_eq!(profile.skin_property, None);

        profile.skin_property = Some("old-texture".to_string());
        profile.update("uuid:two".to_string(), None, None);
        assert_eq!(profile.skin_property, None);
    }

    #[test]
    fn entity_profile_survives_tab_removal() {
        let mut profile =
            PlayerSkinProfileCache::new("uuid:npc".to_string(), None, Some("npc-texture"));

        profile.update("uuid:npc".to_string(), None, Some("npc-texture"));

        assert_eq!(profile.skin_property.as_deref(), Some("npc-texture"));
    }
}

pub struct App {
    pub(super) renderer: Option<crate::render::GameRenderer<super::RenderBackend>>,
    pub(super) window: Option<Window>,
    pub(super) player: client::player::Player,
    pub(super) keybinds: KeyBindings,
    pub(super) input: InputState,
    pub(super) gamepad: GamepadInput,
    pub(super) inventory: Inventory,
    pub(super) inventory_open: bool,
    pub(super) player_list_open: bool,
    pub(super) chat_open: bool,
    pub(super) chat_input: String,
    pub(super) chat_history: Vec<String>,
    pub(super) chat_history_index: Option<usize>,
    pub(super) chat_draft: Option<String>,
    pub(super) book_editor: Option<client::book::BookEditor>,
    pub(super) ctrl_held: bool,
    pub(super) rebinding_action: Option<Action>,
    pub(super) control_device: ControlDevice,
    pub(super) mouse_captured: bool,
    pub(super) attack_held: bool,
    pub(super) use_held: bool,
    /// Right-click press events accumulated since the previous fixed tick.
    /// Vanilla queues KeyBinding presses and consumes them inside runTick.
    pub(super) use_presses_pending: u32,
    /// Attack click events accumulated since the previous fixed tick.
    /// Vanilla clickMouse is only called inside runTick (20 Hz), so rapid
    /// mouse clicks are coalesced to at most one entity attack per tick.
    pub(super) pending_attacks: u8,
    /// Container click-window packets queued from the render-frame callback.
    /// Vanilla GuiContainer.mouseClicked sends C0E inside runTick (20 Hz),
    /// so rapid inventory clicks must be deferred to the tick boundary.
    pub(super) pending_click_windows: Vec<Vec<u8>>,
    /// Vanilla isSwingInProgress — guards C0A animation packets so they are
    /// not sent more than once per 3 ticks (getArmSwingAnimationEnd / 2 = 3).
    pub(super) is_swing_in_progress: bool,
    /// Vanilla swingProgressInt — countdown from -1 through getArmSwingAnimationEnd (6).
    pub(super) swing_progress_int: i32,
    /// Prevent a held controller attack trigger from turning into block mining
    /// after it has already attacked an entity.
    pub(super) gamepad_attack_consumed: bool,
    pub(super) prev_attack_held: bool,
    /// Timer for continuous item use (eating, drinking, bow drawing) in seconds.
    pub(super) item_use_timer: f32,
    pub(super) prev_item_use_timer: f32,
    pub(super) fov_target: f32,
    /// Cooldown ticks after eating completes before next auto-start (vanilla: ~4 ticks gap).
    pub(super) food_cooldown: u8,
    /// Whether an item use action (eating/bow) is currently in progress.
    pub(super) item_use_active: bool,
    /// A C07 release was sent since the previous game tick. Interaction input
    /// in this interval must not append C02/C08 to the release packet.
    pub(super) use_release_pending: bool,
    /// MCP: rightClickDelayTimer — 4 tick cooldown after every right-click.
    pub(super) item_place_delay: u8,
    /// MCP PlayerControllerMP.blockHitDelay — five ticks after a block finish.
    pub(super) block_hit_delay: u8,
    /// Digging cancellation requested by render/input callbacks. It is flushed
    /// during the next fixed tick before the movement packet.
    pub(super) pending_dig_cancel: Option<(
        crate::client::physics::BlockPos,
        crate::client::physics::BlockFace,
    )>,
    /// Tick cooldown for local ladder/vine climbing sounds.
    pub(super) ladder_sound_cooldown: u8,
    pub(super) dig: DigController,
    pub(super) inventory_action_number: i16,
    pub(super) connection: Option<net::connection::Connection>,
    pub(super) network_state: client::network::ClientNetworkState,
    pub(super) audio: audio::AudioBackendImpl,
    pub(super) particles: client::particles::ParticleSystem,
    pub(super) local_player_model: client::player_model::PlayerModel,
    pub(super) local_skin: assets::skin::PlayerSkin,
    pub(super) local_skin_dirty: bool,
    pub(super) local_cape_pixels: Option<Vec<u8>>,
    pub(super) local_cape_hash: u64,
    pub(super) skin_cache: client::skin_cache::SkinCache,
    pub(super) servers: client::server_list::ServerList,
    pub(super) server_refresh_task: Option<tasks::ServerRefreshTask>,
    pub(super) connect_task: Option<tasks::ConnectTask>,
    pub(super) auth_task: Option<tasks::AuthTask>,
    pub(super) account: Option<crate::auth::models::Account>,
    pub(super) accounts: Vec<crate::auth::models::Account>,
    pub(super) account_faces_dirty: bool,
    pub(super) selected_account: usize,
    pub(super) auth_status: String,
    pub(super) offline_username_input: String,
    pub(super) entering_offline_name: bool,
    pub(super) session: client::session::SessionState,
    pub(super) entities: crate::entity::EntityManager,
    pub(super) scripts: crate::scripting::ScriptManager,
    pub(super) config: client::config::ClientConfig,
    pub(super) selected_server: usize,
    pub(super) server_address: String,
    pub(super) server_editor_name: String,
    pub(super) server_editor_address: String,
    pub(super) server_editor_address_focused: bool,
    pub(super) last_server_click: Option<(usize, std::time::Instant)>,
    pub(super) username: String,
    pub(super) tick_timer: TickTimer,
    pub(super) fps_timer: std::time::Instant,
    pub(super) fps_frames: u32,
    pub(super) frame_count: u64,
    pub(super) last_redraw_request: std::time::Instant,
    pub(super) last_script_frame: std::time::Instant,
    pub(super) last_script_hud_frame: std::time::Instant,
    pub(super) last_script_api_frame: std::time::Instant,
    /// Player block and nearby chunk generations backing the cached Lua block volume.
    pub(super) last_script_world_block_key:
        Option<((i32, i32, i32), world::SnapshotRegionRevision)>,
    /// Vanilla `GameSettings.hideGUI`, toggled by F1 while playing.
    pub(super) hud_hidden: bool,
    pub(super) script_visual_overrides: BTreeMap<String, script_bridge::ScriptVisualOverrides>,
    pub(super) script_base_camera_mode: Option<u8>,
    pub(super) world: world::World,
    /// Latest completed mesh per chunk, drained to the renderer under a frame
    /// upload budget so dense server chunk bursts do not stall the event loop.
    pub(super) pending_chunk_uploads: fnv::FnvHashMap<(i32, i32), world::mesh::ChunkMesh>,
    pub(super) state: GameState,
    pub(super) ui: ui::UiState,
    pub(super) mouse_dx: f64,
    pub(super) mouse_dy: f64,
    pub(super) mouse_x: f64,
    pub(super) mouse_y: f64,
    /// The option slider currently held by the left mouse button.
    pub(super) active_slider: Option<u32>,
    /// Whether the vanilla creative-inventory scrollbar is being dragged.
    pub(super) creative_scroll_dragging: bool,
    /// Controller quick-craft candidate: button and protocol slots visited while held.
    pub(super) gamepad_inventory_drag: Option<(winit::event::MouseButton, Vec<i16>)>,
    pub(super) resource_pack_list: Vec<ResourcePackInfo>,
    pub(super) last_sent_abilities: Option<(u8, f32, f32)>,
    pub(super) last_entity_hash: u64,
    pub(super) last_static_render_tick: u64,
    pub(super) last_ui_text_hash: u64,
    pub(super) last_chat_generation: u64,
    pub(super) last_chat_skin_generation: u64,
    pub(super) last_chat_profile_generation: u64,
    pub(super) last_chat_locale: String,
    pub(super) last_chest_structure_generation: u64,
    pub(super) last_sign_hash: u64,
    pub(super) last_skull_hash: u64,
    pub(super) last_player_list_generation: u64,
    pub(super) last_player_list_skin_generation: u64,
    pub(super) last_player_list_avatar_setting: Option<bool>,
    pub(super) last_scoreboard_generation: u64,
    pub(super) last_player_skin_roster_hash: u64,
    pub(super) last_player_skin_generation: u64,
    pub(super) last_player_skin_skull_hash: u64,
    pub(super) last_skin_cache_generation: u64,
    pub(super) player_skin_slims: fnv::FnvHashMap<i32, bool>,
    pub(super) player_cape_ready: fnv::FnvHashSet<i32>,
    pub(super) player_skin_profiles: fnv::FnvHashMap<i32, PlayerSkinProfileCache>,
    pub(super) modding_status: String,
    pub(super) modding_selected: usize,
    pub(super) mod_config_selected: usize,
    pub(super) mod_config_status: String,
    /// Loaded manifest + navigation state for the shader options screen.
    pub(super) shader_options: Option<menu_actions::ShaderOptionsSession>,
    /// Shader-pack GPU setup is deliberately deferred until after one loading
    /// frame has been presented, so a compile cannot make the menu look frozen.
    pub(super) pending_shader_pack_load: Option<PendingShaderPackLoad>,
    pub(super) shader_pack_load_frame_submitted: bool,
    pub(super) last_frame_profile: AppFrameProfile,
    pub(super) last_profile_frame_start: Option<std::time::Instant>,
    pub(super) next_predicted_entity_id: i32,
}

impl App {
    pub fn new() -> Self {
        log::debug!("initialising client application state");
        let mut config = client::config::ClientConfig::load_default();
        log::info!(
            "client settings: language={}, gui_scale={}, render_distance={}, smooth_lighting={}, particles={:?}, fov={:.1}, max_framerate={}, resource_packs={}, audio_device={}",
            config.language,
            config.gui_scale,
            config.render_distance,
            config.smooth_lighting,
            config.particles,
            config.fov,
            config.max_framerate,
            config.enabled_resource_packs.len(),
            config.audio_device
        );
        let account = crate::auth::cache::load_account().ok().flatten();
        let accounts = crate::auth::cache::load_accounts().unwrap_or_default();
        log::info!(
            "account cache: saved_accounts={}, active_account={}",
            accounts.len(),
            if account.is_some() { "present" } else { "none" }
        );
        let selected_account = account
            .as_ref()
            .and_then(|selected| accounts.iter().position(|item| item.uuid == selected.uuid))
            .unwrap_or(0);
        let authenticated_username = account
            .as_ref()
            .and_then(|account| account.username.clone());
        let mut world = world::World::new();
        world.set_smooth_lighting(config.smooth_lighting);
        let spawn_y = 64.0;
        let mut local_player_model = client::player_model::PlayerModel::steve();
        let local_skin = load_local_skin(account.as_ref(), &mut local_player_model);
        let local_cape_raw = account.as_ref().and_then(load_local_cape_pixels);
        let local_cape_hash = local_cape_raw.as_ref().map_or(0, |raw| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash_slice(raw, &mut h);
            std::hash::Hasher::finish(&h)
        });
        let skin_cache = client::skin_cache::SkinCache::new(&local_skin);
        // Audio uses the same enabled resource-pack stack as textures.
        let mut audio_resolver = assets::resolver::AssetResolver::with_resource_packs(
            "assets/minecraft",
            "resourcepacks",
            &config.enabled_resource_packs,
        );
        let mut audio =
            match ResourcePack::load_with_resource_packs("assets", "1.8", &mut audio_resolver) {
                Ok(pack) => {
                    log::info!(
                        "audio resource pack ready: assets={}, sound_events={}",
                        pack.index.len(),
                        pack.sounds.len()
                    );
                    audio::AudioBackendImpl::new(pack.index, pack.sounds)
                }
                Err(e) => {
                    log::error!("failed to load audio resource pack: {e}; using null audio");
                    audio::AudioBackendImpl::new_null()
                }
            };
        apply_audio_config(&mut audio, &config);
        let mut scripts = crate::scripting::ScriptManager::new("mods");
        let script_report = scripts.load_all();
        // Restore persisted enabled/disabled state, then prune missing mods.
        {
            let saved = config.mod_enabled.clone();
            for info in scripts.loaded_mods() {
                if let Some(&enabled) = saved.get(&info.id) {
                    if enabled != info.enabled {
                        let _ = if enabled {
                            scripts.enable(&info.id)
                        } else {
                            scripts.disable(&info.id)
                        };
                    }
                }
            }
            let known_ids: std::collections::HashSet<_> =
                scripts.loaded_mods().into_iter().map(|m| m.id).collect();
            config
                .mod_enabled
                .retain(|id, _| known_ids.contains(id.as_str()));
            config.save_default();
        }
        for id in &script_report.loaded {
            log::info!(target: "rustcraft::lua", "loaded mod '{id}'");
        }
        for (id, permission) in &script_report.denied_permissions {
            log::warn!(
                target: "rustcraft::lua",
                "mod '{id}' denied sensitive permission '{permission}'"
            );
        }
        for error in &script_report.errors {
            log::error!(target: "rustcraft::lua", "mod load failed: {error}");
        }
        log::info!(
            target: "rustcraft::lua",
            "Lua startup complete: loaded={}, denied_permissions={}, errors={}",
            script_report.loaded.len(),
            script_report.denied_permissions.len(),
            script_report.errors.len()
        );
        let modding_status = format!("{} mod(s) loaded", scripts.mod_count());

        App {
            renderer: None,
            window: None,
            player: client::player::Player::new(
                nalgebra::Point3::new(8.0, spawn_y, 25.0),
                1280.0 / 720.0,
            ),
            keybinds: config.keybinds.clone(),
            input: InputState::new(),
            gamepad: GamepadInput::new(),
            inventory: Inventory::creative(),
            inventory_open: false,
            player_list_open: false,
            chat_open: false,
            chat_input: String::new(),
            chat_history: Vec::new(),
            chat_history_index: None,
            chat_draft: None,
            book_editor: None,
            ctrl_held: false,
            rebinding_action: None,
            control_device: ControlDevice::KeyboardMouse,
            mouse_captured: false,
            attack_held: false,
            use_held: false,
            use_presses_pending: 0,
            pending_attacks: 0,
            pending_click_windows: Vec::new(),
            is_swing_in_progress: false,
            swing_progress_int: 0,
            gamepad_attack_consumed: false,
            prev_attack_held: false,
            item_use_timer: 0.0,
            prev_item_use_timer: 0.0,
            fov_target: 1.0,
            food_cooldown: 0,
            item_use_active: false,
            use_release_pending: false,
            item_place_delay: 0,
            block_hit_delay: 0,
            pending_dig_cancel: None,
            ladder_sound_cooldown: 0,
            dig: DigController::new(),
            inventory_action_number: 1,
            connection: None,
            network_state: client::network::ClientNetworkState::new(),
            audio,
            particles: client::particles::ParticleSystem::new(4096),
            local_player_model,
            local_skin,
            local_skin_dirty: true,
            local_cape_pixels: local_cape_raw,
            local_cape_hash,
            skin_cache,
            servers: client::server_list::ServerList::load_default(),
            server_refresh_task: None,
            connect_task: None,
            auth_task: None,
            account,
            accounts,
            account_faces_dirty: true,
            selected_account,
            auth_status: if authenticated_username.is_some() {
                "Microsoft account loaded from cache".to_string()
            } else {
                "No Microsoft account selected".to_string()
            },
            offline_username_input: String::new(),
            entering_offline_name: false,
            session: client::session::SessionState::new(),
            entities: crate::entity::EntityManager::new(),
            scripts,
            selected_server: 0,
            // A direct-connect address is transient UI state, not a game setting.
            // Keep the environment override useful for development without writing it
            // into options.json.
            server_address: std::env::var("RUSTCRAFT_SERVER")
                .unwrap_or_else(|_| "127.0.0.1:25565".to_string()),
            server_editor_name: String::new(),
            server_editor_address: String::new(),
            server_editor_address_focused: false,
            last_server_click: None,
            username: authenticated_username.unwrap_or_else(|| config.username.clone()),
            tick_timer: TickTimer::new(),
            fps_timer: std::time::Instant::now(),
            fps_frames: 0,
            frame_count: 0,
            last_redraw_request: std::time::Instant::now(),
            last_script_frame: std::time::Instant::now(),
            last_script_hud_frame: std::time::Instant::now(),
            last_script_api_frame: std::time::Instant::now(),
            last_script_world_block_key: None,
            hud_hidden: false,
            script_visual_overrides: BTreeMap::new(),
            script_base_camera_mode: None,
            world,
            pending_chunk_uploads: fnv::FnvHashMap::default(),
            state: GameState::MainMenu,
            ui: ui::UiState::new(&config.language),
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            active_slider: None,
            creative_scroll_dragging: false,
            gamepad_inventory_drag: None,
            resource_pack_list: scan_resource_packs(&config.enabled_resource_packs),
            last_sent_abilities: None,
            last_entity_hash: 0,
            last_static_render_tick: u64::MAX,
            last_ui_text_hash: 0,
            last_chat_generation: u64::MAX,
            last_chat_skin_generation: u64::MAX,
            last_chat_profile_generation: u64::MAX,
            last_chat_locale: String::new(),
            last_chest_structure_generation: u64::MAX,
            last_sign_hash: 0,
            last_skull_hash: 0,
            last_player_list_generation: u64::MAX,
            last_player_list_skin_generation: u64::MAX,
            last_player_list_avatar_setting: None,
            last_scoreboard_generation: u64::MAX,
            last_player_skin_roster_hash: u64::MAX,
            last_player_skin_generation: u64::MAX,
            last_player_skin_skull_hash: u64::MAX,
            last_skin_cache_generation: u64::MAX,
            player_skin_slims: fnv::FnvHashMap::default(),
            player_cape_ready: fnv::FnvHashSet::default(),
            player_skin_profiles: fnv::FnvHashMap::default(),
            modding_status,
            modding_selected: 0,
            mod_config_selected: 0,
            mod_config_status: String::new(),
            shader_options: None,
            pending_shader_pack_load: None,
            shader_pack_load_frame_submitted: false,
            last_frame_profile: AppFrameProfile::default(),
            last_profile_frame_start: None,
            next_predicted_entity_id: -1,
            config,
        }
    }
}
