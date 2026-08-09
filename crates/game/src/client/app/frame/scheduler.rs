use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

fn next_chunk_upload_coord(
    pending: &fnv::FnvHashMap<(i32, i32), crate::world::mesh::ChunkMesh>,
    player_chunk: (i32, i32),
) -> Option<(i32, i32)> {
    pending.keys().copied().min_by_key(|&(cx, cz)| {
        let dx = i128::from(cx) - i128::from(player_chunk.0);
        let dz = i128::from(cz) - i128::from(player_chunk.1);
        (dx * dx + dz * dz, cx, cz)
    })
}

impl App {
    pub(crate) fn handle_redraw(&mut self, event_loop: &ActiveEventLoop) {
        let frame_started = std::time::Instant::now();
        let frame_interval_us = self
            .last_profile_frame_start
            .replace(frame_started)
            .map(|previous| frame_started.duration_since(previous).as_micros() as u64)
            .unwrap_or(0);
        let published = self.last_frame_profile;
        if let Some(renderer) = &mut self.renderer {
            renderer.frame.frame_total_us = published.total_us;
            renderer.frame.frame_interval_us = frame_interval_us;
            renderer.frame.frame_outside_us = frame_interval_us.saturating_sub(published.total_us);
            renderer.frame.frame_tasks_us = published.tasks_us;
            renderer.frame.frame_network_us = published.network_us;
            renderer.frame.frame_world_us = published.world_us;
            renderer.frame.frame_tick_us = published.tick_us;
            renderer.frame.frame_sync_us = published.sync_us;
            renderer.frame.frame_render_us = published.render_us;
            renderer.frame.frame_other_us = published.other_us;
            renderer.frame.frame_script_us = published.script_us;
            renderer.frame.frame_script_callbacks = published.script_callbacks;
            renderer.frame.frame_script_slow_callbacks = published.script_slow_callbacks;
            renderer.frame.frame_network_debug = published.network_debug;
            // Only the Playing state guarantees the window is focused (unfocus
            // auto-pauses), so it's the sole state that should feed 1% low.
            renderer.frame.frame_interval_in_gameplay = matches!(self.state, GameState::Playing);
            renderer.frame.snapshot_completed_frame_profile(
                renderer.settings.max_framerate,
                renderer.world.chunk_count_loaded,
            );
            renderer.frame.reset_current_frame_profile();
        }

        // Let the OS/compositor present one complete loading frame before the
        // synchronous shader compiler and renderer resource rebuild begin.
        if self.shader_pack_load_frame_submitted {
            self.shader_pack_load_frame_submitted = false;
            self.process_pending_shader_pack_load();
        }

        let tasks_started = std::time::Instant::now();
        self.poll_background_tasks();
        let tasks_us = tasks_started.elapsed().as_micros() as u64;

        let network_started = std::time::Instant::now();
        let network_result = if self.connection.is_some() {
            client::network::poll_network(
                &mut self.connection,
                &mut self.scripts,
                &mut self.player,
                &mut self.network_state,
                &mut self.world,
                &mut self.inventory,
                &mut self.session,
                &mut self.entities,
                &mut self.particles,
                &mut self.audio,
                Some(&self.ui.i18n),
            )
        } else {
            client::network::NetworkPollResult::default()
        };
        let network_debug = network_result.debug;
        let updated_meshes = network_result.meshes;
        let network_us = network_started.elapsed().as_micros() as u64;

        let world_started = std::time::Instant::now();
        if !updated_meshes.is_empty() {
            if let Some(renderer) = &mut self.renderer {
                renderer.upload_world_partial(&updated_meshes);
            }
        }

        // LoadingWorld → Playing: transition when server sends initial position
        if matches!(self.state, GameState::LoadingWorld) {
            if self.session.received_initial_position {
                let loaded = self.world.chunks.len();
                let rd = self.config.render_distance as i32;
                let expected = (rd * 2 + 1).pow(2) as usize;
                log::info!(
                    "world ready: loaded_chunks={}, expected_chunks={}, initial_position_received=true",
                    loaded, expected
                );
                self.world.defer_mesh_build = false;
                self.state = GameState::Playing;
                // Enqueue already-received chunks for incremental mesh building.
                // Don't bulk-build them all now — that causes a multi-frame stutter.
                // The incremental builder below handles 4 at a time.
                self.world.enqueue_all_chunks_for_mesh();
                self.set_cursor_captured(true);
                self.mouse_captured = true;
            }
        }

        // Playing → Disconnected: detect connection loss
        if matches!(self.state, GameState::Playing) {
            let still_connected = self
                .connection
                .as_ref()
                .map(|c| c.connected.load(std::sync::atomic::Ordering::SeqCst))
                .unwrap_or(false);
            if !still_connected && self.connection.is_some() {
                let reason = self
                    .session
                    .last_disconnect_reason
                    .clone()
                    .unwrap_or_else(|| self.ui.t("rustcraft.connection.disconnected"));
                self.disconnect_world(&reason);
            }
        }
        if matches!(self.state, GameState::Playing) {
            let player_chunk = (
                (self.player.position.x / 16.0).floor() as i32,
                (self.player.position.z / 16.0).floor() as i32,
            );
            for mesh in self
                .world
                .poll_finished_meshes()
                .into_iter()
                .chain(self.world.schedule_background_meshes())
            {
                self.pending_chunk_uploads.insert((mesh.cx, mesh.cz), mesh);
            }

            // Building GPU draw metadata and staging dense chunks happens on
            // this event/render thread. Cap both byte volume and wall-clock
            // time so a burst of dense chunks cannot hitch the frame.
            // Chunk uploads happen on the event/render thread. Keep the
            // burst small enough that a newly-arrived section cannot turn a
            // 1000-FPS redraw loop into an 8ms hitch.
            const CHUNK_UPLOAD_BUDGET: usize = 2 * 1024 * 1024;
            /// Even a single mesh larger than this is deferred to the next
            /// frame so one ultra-dense chunk can't monopolise a frame.
            const CHUNK_UPLOAD_SINGLE_MAX: usize = 1 * 1024 * 1024;
            /// Wall-clock budget for the upload-staging path. Anything that
            /// doesn't fit is left in `pending_chunk_uploads` for next frame.
            const CHUNK_UPLOAD_TIME_BUDGET_US: u64 = 2_000;
            let upload_started = std::time::Instant::now();
            let mut upload = Vec::new();
            let mut upload_bytes = 0usize;
            while let Some(coord) =
                next_chunk_upload_coord(&self.pending_chunk_uploads, player_chunk)
            {
                // Peek the size without removing so we can defer oversize or
                // over-budget meshes without re-inserting them.
                let mesh_bytes = {
                    let mesh = self.pending_chunk_uploads.get(&coord).unwrap();
                    mesh.vertices.len() * std::mem::size_of::<crate::world::mesh::Vertex>()
                        + mesh.indices.len() * std::mem::size_of::<u32>()
                };
                if (!upload.is_empty() && mesh_bytes > CHUNK_UPLOAD_SINGLE_MAX)
                    || (!upload.is_empty() && upload_bytes + mesh_bytes > CHUNK_UPLOAD_BUDGET)
                    || (!upload.is_empty()
                        && upload_started.elapsed().as_micros() as u64
                            > CHUNK_UPLOAD_TIME_BUDGET_US)
                {
                    break;
                }
                let mesh = self.pending_chunk_uploads.remove(&coord).unwrap();
                upload_bytes += mesh_bytes;
                upload.push(mesh);
            }
            if !upload.is_empty() {
                if let Some(renderer) = &mut self.renderer {
                    renderer.upload_world_partial(&upload);
                }
            }

            // Evict chunks outside render distance to cap memory
            let rd = self.config.render_distance;
            let evicted =
                self.world
                    .unload_distant_chunks(player_chunk.0, player_chunk.1, rd as u32);
            if !evicted.is_empty() {
                if let Some(renderer) = &mut self.renderer {
                    renderer.upload_world_partial(&evicted);
                }
            }

            // Trim snapshot tombstones every 100 frames
            self.frame_count = self.frame_count.wrapping_add(1);
            if self.frame_count % 100 == 0 {
                self.world.trim_snapshot_tombstones();
            }
        }
        let world_us = world_started.elapsed().as_micros() as u64;

        if self.inventory.window_just_opened {
            self.inventory.window_just_opened = false;
            self.inventory_open = true;
            self.mouse_captured = false;
            self.set_cursor_captured(false);
        }
        // Auto-close inventory UI when the server closes our container window
        // (open_window_id drops from non-zero to 0), but NOT when user opens
        // their own player inventory (window_id=0).
        if self.inventory_open
            && self.inventory.open_window_id == 0
            && self.inventory.had_server_window
        {
            // The server initiated the close (S2E CloseWindow).  We must still
            // decrement the local chest viewer count so the lid closes; the
            // S2E handler cannot do this because it has no World access.
            if let Some(position) = self.inventory.open_chest_position.take() {
                self.world.close_chest_for_local_viewer(position);
            }
            self.inventory_open = false;
            self.inventory.had_server_window = false;
            self.inventory.cursor = crate::client::inventory::ItemStack::EMPTY;
            self.mouse_captured = true;
            self.set_cursor_captured(true);
        }
        if self.inventory.open_window_id != 0 {
            self.inventory.had_server_window = true;
        }

        if matches!(self.state, GameState::Playing)
            && self.mouse_captured
            && self
                .session
                .resource_pack
                .as_ref()
                .is_some_and(|pack| pack.status == "available")
        {
            self.mouse_captured = false;
            self.set_cursor_captured(false);
        }

        let tick_started = std::time::Instant::now();
        self.update_fps_and_title();
        self.tick_gameplay(event_loop);
        let tick_us = tick_started.elapsed().as_micros() as u64;
        // Camera FOV and view-bobbing are set from config / script overrides.
        // The tick loop only runs when tick_timer advances, so updating them
        // here ensures they are fresh every frame — otherwise a FOV change in
        // the options menu takes effect only on the next tick (up to 50ms lag).
        self.player.camera.fov = self.script_effective_fov();
        self.player.camera.view_bobbing = self.script_effective_view_bobbing();
        let t_sync = std::time::Instant::now();
        self.sync_renderer_state();
        let sync_us = t_sync.elapsed().as_micros() as u64;

        let hurt_cam = self.script_effective_hurt_cam();
        let fov_change = self.script_effective_fov_change();

        let mut render_us = 0;
        if let Some(renderer) = &mut self.renderer {
            let eye = self.player.camera.position;
            let underwater = matches!(
                self.world
                    .get_block(eye.x as i32, eye.y as i32, eye.z as i32),
                crate::world::block::Block::FlowingWater | crate::world::block::Block::StillWater
            );
            renderer.world.underwater = underwater;
            renderer.world.underwater_yaw = self.player.camera.yaw;
            renderer.world.underwater_pitch = self.player.camera.pitch;
            self.player.camera.hurt_cam_enabled = hurt_cam;
            self.player.camera.fov_change_enabled = fov_change;
            let mut fov_mod = 1.0f32;
            if self.player.camera.fov_change_enabled {
                if underwater {
                    fov_mod *= 60.0 / 70.0;
                }
                if self.player.flying {
                    fov_mod *= 1.1;
                }
                if self.player.sprinting {
                    fov_mod *= 1.15;
                }
                if self.item_use_active {
                    let held_id = self.inventory.selected_item().item_id;
                    if held_id == 261 {
                        let charge = (self.item_use_timer / 1.0).min(1.0);
                        fov_mod *= 1.0 - charge * charge * 0.15;
                    }
                }
            }
            // Apply as target; actual smoothing happens in physics tick
            self.fov_target = fov_mod;
            let render_started = std::time::Instant::now();
            self.player.camera.partial_tick = self.tick_timer.alpha();
            renderer.draw_frame(
                &self.player.camera,
                self.state.menu_id(),
                underwater,
                self.state.has_world_background(),
            );
            render_us = render_started.elapsed().as_micros() as u64;
        }

        // Defer the actual load until the next redraw. This gives the previous
        // queue-present call a chance to reach the compositor, even when
        // shaderc compilation is CPU-bound.
        if self.pending_shader_pack_load.is_some() {
            self.shader_pack_load_frame_submitted = true;
        }

        let (script_us, script_callbacks, script_slow_callbacks) =
            self.scripts.take_frame_profile();
        let total_us = frame_started.elapsed().as_micros() as u64;
        let accounted_us = tasks_us
            .saturating_add(network_us)
            .saturating_add(world_us)
            .saturating_add(tick_us)
            .saturating_add(sync_us)
            .saturating_add(render_us);
        self.last_frame_profile = crate::client::app::AppFrameProfile {
            total_us,
            tasks_us,
            network_us,
            world_us,
            tick_us,
            sync_us,
            render_us,
            other_us: total_us.saturating_sub(accounted_us),
            script_us,
            script_callbacks,
            script_slow_callbacks,
            network_debug,
        };
    }

    fn process_pending_shader_pack_load(&mut self) {
        let Some(request) = self.pending_shader_pack_load.take() else {
            return;
        };
        let Some(renderer) = &mut self.renderer else {
            return;
        };

        let result = renderer.load_optifine_shader_pack(&request.pack, &request.overrides);
        renderer.screens.shader_pack_status = match (request.reason, result) {
            (crate::client::app::ShaderPackLoadReason::Selection, Ok(())) => {
                format!("Active: {}", request.pack)
            }
            (crate::client::app::ShaderPackLoadReason::Selection, Err(error)) => {
                format!("Selected {}; {}", request.pack, error)
            }
            (crate::client::app::ShaderPackLoadReason::OptionsReload, Ok(())) => {
                format!("Active: {}", request.pack)
            }
            (crate::client::app::ShaderPackLoadReason::OptionsReload, Err(error)) => {
                format!("Shader reload failed: {error}")
            }
        };
        renderer.screens.shader_pack_loading = false;
        renderer.screens.shader_pack_loading_name.clear();
        renderer.screens.shader_pack_loading_stage.clear();
        renderer.invalidate_gui();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn poll_background_tasks(&mut self) {
        let authenticated = self.auth_task.as_ref().and_then(|task| task.try_finish());
        if let Some(result) = authenticated {
            self.auth_task = None;
            match result {
                Ok(account) => {
                    self.username = account
                        .username
                        .clone()
                        .unwrap_or_else(|| self.username.clone());
                    self.config.username = self.username.clone();
                    self.config.save_default();
                    self.auth_status = "Microsoft account authenticated".to_string();
                    self.account = Some(account);
                    self.update_local_skin();
                    self.accounts = crate::auth::cache::load_accounts().unwrap_or_default();
                    self.account_faces_dirty = true;
                    self.selected_account = self
                        .accounts
                        .iter()
                        .position(|saved| {
                            self.account
                                .as_ref()
                                .is_some_and(|active| active.uuid == saved.uuid)
                        })
                        .unwrap_or(0);
                }
                Err(error) => self.auth_status = format!("Login failed: {error}"),
            }
        }
        let refreshed = self
            .server_refresh_task
            .as_ref()
            .and_then(|task| task.try_finish());
        if let Some(servers) = refreshed {
            self.servers = servers;
            self.selected_server = self
                .selected_server
                .min(self.servers.servers.len().saturating_sub(1));
            self.server_refresh_task = None;
            self.session
                .push_system_line(self.ui.t("rustcraft.server.refreshed"));
        }

        let connected = self.connect_task.as_ref().and_then(|task| {
            task.try_finish()
                .map(|result| (task.address.clone(), result))
        });
        if let Some((addr, result)) = connected {
            self.connect_task = None;
            match result {
                Ok(conn) => {
                    log::info!("connection task completed successfully: server={addr}");
                    self.selected_server = self.servers.upsert(addr.clone(), addr.clone());
                    self.connection = Some(conn);
                    self.scripts.set_connection_active(true);
                    self.network_state = crate::client::network::ClientNetworkState::new();
                    self.session.push_system_line(format!(
                        "{}: {}",
                        self.ui.t("rustcraft.connection.connected"),
                        addr
                    ));
                    if matches!(self.state, GameState::Connecting) {
                        self.state = GameState::LoadingWorld;
                        self.session.received_initial_position = false;
                        self.world.defer_mesh_build = true;
                        self.world.set_smooth_lighting(self.config.smooth_lighting);
                        let day_time = self.session.day_time as f32;
                        self.world.set_sky_brightness(
                            renderer_api::core::sky::SkyGradient::daylight_factor(day_time),
                        );
                        self.tick_timer = TickTimer::new();
                    }
                }
                Err(err) => {
                    log::error!("connection task failed: server={addr}, error={err}");
                    self.session.push_system_line(self.ui.i18n.tf(
                        "rustcraft.connection.failedWithReason",
                        &[&addr, &err.to_string()],
                    ));
                    self.attack_held = false;
                    self.use_held = false;
                    self.use_presses_pending = 0;
                    self.pending_attacks = 0;
                    self.dig.cancel();
                    self.pending_dig_cancel = None;
                    self.block_hit_delay = 0;
                    self.state = GameState::Multiplayer;
                    self.mouse_captured = false;
                    self.set_cursor_captured(false);
                }
            }
        }
    }

    fn update_fps_and_title(&mut self) {
        self.fps_frames += 1;
        if self.fps_timer.elapsed().as_secs_f32() < 1.0 {
            return;
        }

        self.ui.update_fps(self.fps_frames);
        if let Some(renderer) = &mut self.renderer {
            renderer.frame.fps_count = self.fps_frames;
        }
        if let Some(window) = &self.window {
            window.set_title(crate::client::app::WINDOW_TITLE);
        }
        self.fps_frames = 0;
        self.fps_timer = std::time::Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::next_chunk_upload_coord;
    use crate::world::mesh::ChunkMesh;

    fn mesh(cx: i32, cz: i32) -> ChunkMesh {
        ChunkMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            cx,
            cz,
            transparent_start: 0,
            aabb_min: [f32::MAX; 3],
            aabb_max: [f32::MIN; 3],
        }
    }

    #[test]
    fn chunk_uploads_nearest_to_the_player_are_selected_first() {
        let mut pending = fnv::FnvHashMap::default();
        for (cx, cz) in [(30, 30), (4, 5), (6, 5), (-20, 2)] {
            pending.insert((cx, cz), mesh(cx, cz));
        }

        assert_eq!(next_chunk_upload_coord(&pending, (5, 5)), Some((4, 5)));
    }
}
