use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

impl App {
    pub(super) fn tick_gameplay(&mut self, event_loop: &ActiveEventLoop) {
        // Gilrs is polling-based, unlike winit's keyboard/mouse event stream.
        // Poll once per rendered frame so controller input stays responsive even
        // when the game has no fixed simulation tick in that frame.
        self.poll_gamepad(event_loop);
        let now = std::time::Instant::now();
        let api_frame_delta = now
            .duration_since(self.last_script_api_frame)
            .as_secs_f32()
            .min(0.5);
        self.last_script_api_frame = now;
        self.publish_script_snapshots(api_frame_delta, false);
        self.apply_script_commands();
        if matches!(self.state, GameState::Playing) && self.session.health <= 0.0 {
            self.attack_held = false;
            self.use_held = false;
            self.use_presses_pending = 0;
            self.pending_attacks = 0;
            self.dig.cancel();
            self.pending_dig_cancel = None;
            self.block_hit_delay = 0;
            if self.mouse_captured {
                self.mouse_captured = false;
                self.set_cursor_captured(false);
            }
        }
        if !matches!(self.state, GameState::Playing) {
            // Keep the world alive when menus are open — skip player input only
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }

        // Mouse input — only when the game has focus
        if self.mouse_captured {
            self.player.process_mouse(
                self.mouse_dx as f32,
                self.mouse_dy as f32,
                self.config.mouse_sensitivity,
                self.config.invert_mouse,
            );
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }

        let physics_ticks = self.tick_timer.update();
        let ticks = physics_ticks;
        let mut interaction_meshes = Vec::new();
        for tick_i in 0..ticks {
            self.tick_timer.begin_tick();
            self.publish_script_snapshots(1.0 / 20.0, true);
            self.scripts.dispatch_json(
                "client.tick",
                serde_json::json!({
                    "tick": self.tick_timer.tick_count(),
                    "delta_time": 1.0 / 20.0,
                    "playing": matches!(self.state, GameState::Playing),
                }),
            );
            self.apply_script_commands();
            for (mod_id, packet) in self.scripts.take_active_packets() {
                let hooked = self
                    .scripts
                    .process_packet("network.packet.outbound", packet);
                let Some(packet) = hooked.packet else {
                    continue;
                };
                if let Err(error) = client::network::send_dynamic_packet(&self.connection, &packet)
                {
                    log::warn!(
                        target: "rustcraft::lua",
                        "mod '{mod_id}' active packet rejected: {error}"
                    );
                }
            }
            // Smooth FOV — save previous for frame interpolation, then LERP
            self.player.camera.prev_fov_modifier = self.player.camera.fov_modifier;
            let target = self.fov_target.clamp(0.1, 1.5);
            self.player.camera.fov_modifier += (target - self.player.camera.fov_modifier) * 0.5;
            // Save prev use_timer for smooth interpolation
            self.prev_item_use_timer = self.item_use_timer;
            // S19 status 9 is handled by the network thread before the next
            // client tick in vanilla. Clear using-item state before movement
            // so the first tick after server confirmation is not slowed.
            if self.player.take_item_use_finished() {
                self.item_use_active = false;
                self.item_use_timer = 0.0;
                self.food_cooldown = 4;
                if self.connection.is_none() {
                    self.inventory.remove_selected_one();
                }
            }
            // Vanilla PlayerControllerMP.updateController synchronizes the
            // selected hotbar slot before any C07/C08 interaction packet.
            client::network::sync_held_item(
                &self.connection,
                &mut self.network_state,
                self.inventory.selected,
            );
            // A render-frame release/use edge may have queued ABORT. Vanilla
            // emits it before the next interaction packet and before movement.
            self.flush_pending_dig_cancel();
            // Vanilla GuiContainer.mouseClicked sends C0E during
            // currentScreen.handleMouseInput() inside runTick, before
            // clickMouse.  Flush any clicks the render-frame callback queued.
            if let Some(connection) = &self.connection {
                for data in self.pending_click_windows.drain(..) {
                    connection.send_play_packet(0x0E, &data);
                }
            }
            // Minecraft.runTick drains attack presses instead of dispatching
            // them when the tick starts with an item already in use.
            let using_item_at_tick_start = self.item_use_active;
            if using_item_at_tick_start {
                self.pending_attacks = 0;
            }
            // In the non-using branch vanilla processes clickMouse before its
            // right-click queue. This matters when both edges occur in one
            // tick: the attack must not be sent after eating/blocking starts.
            if self.is_swing_in_progress {
                self.swing_progress_int += 1;
                if self.swing_progress_int >= 6 {
                    self.is_swing_in_progress = false;
                    self.swing_progress_int = 0;
                }
            }
            if self.pending_attacks > 0 {
                self.pending_attacks -= 1;
                if self.attack_targeted_entity() {
                    self.attack_held = false;
                }
            }
            // Consume queued right-click input after clickMouse and before
            // continuous mining, matching Minecraft.runTick.
            self.tick_item_use_input();

            // Minecraft.sendClickBlockToController runs before world entities
            // update, so digging packets use the pre-movement position/yaw.
            interaction_meshes.extend(self.tick_block_interaction());
            // Player movement: always tick for gravity/friction.
            // When the mouse isn't captured (menu open), pass neutral input
            // so the player doesn't move but still falls/lands naturally.
            self.player.using_item = self.item_use_active;
            if self.mouse_captured {
                self.player.tick(
                    &self.input,
                    &self.world,
                    &self.entities,
                    self.session.entity_id,
                );
            } else {
                self.player.tick(
                    &InputState::new(),
                    &self.world,
                    &self.entities,
                    self.session.entity_id,
                );
            }
            self.tick_climbing_sound();
            // Update spatial audio listener position from player camera
            let pos = self.player.camera.position;
            let yaw = self.player.camera.yaw;
            self.audio
                .set_listener([pos[0] as f32, pos[1] as f32, pos[2] as f32], yaw);
            self.world.tick_chests();
            self.entities.tick_all(1.0 / 20.0, &self.world);
            // Spawn entity-specific particles (mob walking, fire, spell effects, etc.)
            self.entities
                .spawn_entity_particles(&mut self.particles, &self.world);
            if self.tick_timer.tick_count() % 4 == 0 {
                for effect in &self.player.active_effects {
                    if !effect.hide_particles {
                        self.particles.spawn_mob_spell(
                            nalgebra::Point3::new(
                                self.player.position.x as f32,
                                self.player.position.y as f32 + 0.5,
                                self.player.position.z as f32,
                            ),
                            crate::entity::potion_effect_color(effect.effect_id),
                            false,
                        );
                    }
                }
            }
            // Spawn environment particles (water/lava drips, underwater, etc.)
            self.spawn_environment_particles();
            if self.session.gamemode == 1 && self.inventory.selected_item().item_id == 166 {
                let mut rng = rand::thread_rng();
                let base = [
                    self.player.position.x.floor() as i32,
                    self.player.position.y.floor() as i32,
                    self.player.position.z.floor() as i32,
                ];
                for _ in 0..1000 {
                    let x = base[0] + rng.gen_range(0..16) - rng.gen_range(0..16);
                    let y = base[1] + rng.gen_range(0..16) - rng.gen_range(0..16);
                    let z = base[2] + rng.gen_range(0..16) - rng.gen_range(0..16);
                    if self.world.get_block(x, y, z) == crate::world::block::Block::Barrier {
                        self.particles
                            .spawn_barrier(nalgebra::Point3::new(x as f32, y as f32, z as f32));
                    }
                }
            }
            self.particles.tick_in_world(1.0 / 20.0, &self.world);
            // Tick item use animation at game-tick rate (eating, drinking, bow, sword)
            if self.item_use_active || self.use_held {
                self.tick_item_use(1.0 / 20.0);
            }
            // Auto-continue eating after cooldown (vanilla: auto-starts next food if held)
            if self.food_cooldown > 0 {
                self.food_cooldown -= 1;
            }
            self.session.tick_title(1);
            self.session.tick_action_bar(1);
            // WorldClient.tick increments total time every client tick and
            // day time only while doDaylightCycle is enabled. S03 periodically
            // corrects both values, but does not replace local ticking.
            if self.session.entity_id.is_some() {
                self.session.tick_world_time();
            }
            if let Some(renderer) = &mut self.renderer {
                renderer.tick_texture_animations(crate::assets::texture::TextureAnimationContext {
                    day_time: self.session.day_time,
                    dimension: self.session.dimension,
                    player_x: self.player.position.x,
                    player_z: self.player.position.z,
                    player_yaw_degrees: f64::from(self.player.camera.mc_yaw_degrees()),
                    spawn_position: self.session.spawn_position,
                });
            }
            let ability_flags = (if self.player.flying { 0x02 } else { 0 })
                | (if self.player.allow_flying { 0x04 } else { 0 });
            let new_abilities = (
                ability_flags,
                self.session.flying_speed,
                self.session.walking_speed,
            );
            if self.last_sent_abilities != Some(new_abilities) {
                client::network::send_player_abilities(
                    &self.connection,
                    ability_flags,
                    self.session.flying_speed,
                    self.session.walking_speed,
                );
                self.last_sent_abilities = Some(new_abilities);
            }
            client::network::send_player_tick(
                &self.connection,
                &mut self.player,
                &mut self.network_state,
                self.session.entity_id,
            );
            self.use_release_pending = false;
            self.input.tick_reset();
        }

        if !interaction_meshes.is_empty() {
            if let Some(renderer) = &mut self.renderer {
                renderer.upload_world_partial(&interaction_meshes);
            }
        }

        self.apply_script_visual_overrides();
        self.player
            .update_render_position(self.tick_timer.alpha(), Some(&self.world));
        self.entities
            .update_render_positions(self.tick_timer.alpha());
    }

    /// Spawn environment particles (water/lava drips, underwater bubbles, etc.)
    fn spawn_environment_particles(&mut self) {
        use crate::world::block::Block;

        let cam_position = self.player.camera.position;
        let px = cam_position.x as i32;
        let py = cam_position.y as i32;
        let pz = cam_position.z as i32;

        self.spawn_rain_particles();

        // Only spawn environment particles occasionally (every 8 ticks)
        if self.tick_timer.tick_count() % 8 != 0 {
            return;
        }

        // Check blocks around the camera for water/lava drips
        for dx in -4..=4 {
            for dy in -3..=3 {
                for dz in -4..=4 {
                    let bx = px + dx;
                    let by = py + dy;
                    let bz = pz + dz;
                    let block = self.world.get_block(bx, by, bz);

                    // Water drip from ceiling
                    if block == Block::StillWater || block == Block::FlowingWater {
                        // Check if there's air above
                        let above = self.world.get_block(bx, by + 1, bz);
                        if above == Block::Air {
                            // Spawn water drip particles below
                            if crate::client::particles::particle_pos_hash(
                                (bx as f32 * 0.37 + bz as f32 * 0.11 + by as f32 * 0.53) * 10.0,
                            ) < 0.02
                            {
                                self.particles.spawn_drip(
                                    nalgebra::Point3::new(
                                        bx as f32 + 0.5,
                                        by as f32,
                                        bz as f32 + 0.5,
                                    ),
                                    false,
                                );
                            }
                        }
                    }

                    // Lava drip from ceiling
                    if block == Block::FlowingLava || block == Block::StillLava {
                        let above = self.world.get_block(bx, by + 1, bz);
                        if above == Block::Air {
                            if crate::client::particles::particle_pos_hash(
                                (bx as f32 * 0.19 + bz as f32 * 0.53 + by as f32 * 0.37) * 10.0,
                            ) < 0.02
                            {
                                self.particles.spawn_drip(
                                    nalgebra::Point3::new(
                                        bx as f32 + 0.5,
                                        by as f32,
                                        bz as f32 + 0.5,
                                    ),
                                    true,
                                );
                            }
                        }
                    }
                }
            }
        }

        // Underwater particles (suspended, bubbles)
        let eye_block = self.world.get_block(px, py, pz);
        if eye_block == Block::StillWater || eye_block == Block::FlowingWater {
            // Spawn suspended particles
            for i in 0..2 {
                let seed = self.tick_timer.tick_count() as f32 + i as f32;
                let ox = crate::client::particles::particle_pos_hash(seed) * 6.0 - 3.0;
                let oy = crate::client::particles::particle_pos_hash(seed + 1.0) * 4.0 - 2.0;
                let oz = crate::client::particles::particle_pos_hash(seed + 2.0) * 6.0 - 3.0;
                self.particles.spawn(crate::client::particles::Particle {
                    kind: crate::client::particles::ParticleKind::Suspended,
                    position: nalgebra::Point3::new(
                        cam_position.x + ox,
                        cam_position.y + oy,
                        cam_position.z + oz,
                    ),
                    velocity: nalgebra::Vector3::new(0.0, 0.01, 0.0),
                    age: 0.0,
                    lifetime: 1.5,
                    size: 0.04,
                    color: [0.40, 0.60, 0.90, 0.25],
                    rotation: 0.0,
                    texture_jitter: [0.0, 0.0],
                    on_ground: false,
                });
            }

            // Spawn bubbles near the player's head
            if self.tick_timer.tick_count() % 16 == 0 {
                let seed = self.tick_timer.tick_count() as f32;
                self.particles.spawn(crate::client::particles::Particle {
                    kind: crate::client::particles::ParticleKind::Bubble,
                    position: nalgebra::Point3::new(
                        cam_position.x + crate::client::particles::particle_pos_hash(seed) * 0.4
                            - 0.2,
                        cam_position.y,
                        cam_position.z
                            + crate::client::particles::particle_pos_hash(seed + 3.0) * 0.4
                            - 0.2,
                    ),
                    velocity: nalgebra::Vector3::new(0.0, 0.05, 0.0),
                    age: 0.0,
                    lifetime: 0.5,
                    size: 0.06,
                    color: [0.40, 0.65, 1.0, 0.50],
                    rotation: 0.0,
                    texture_jitter: [0.0, 0.0],
                    on_ground: false,
                });
            }
        }
    }

    /// Local precipitation follows the player's camera instead of being a HUD
    /// indicator. The world collision pass stops drops under ceilings.
    fn spawn_rain_particles(&mut self) {
        if !self.config.weather_effects || !self.session.game_state.raining {
            return;
        }

        let camera = &self.player.camera;
        let tick = self.tick_timer.tick_count() as f32;
        let strength = self.session.game_state.rain_level.max(0.1).clamp(0.0, 1.0);
        for index in 0..8 {
            let seed = tick * 17.0 + index as f32 * 5.0;
            let x =
                camera.position.x + crate::client::particles::particle_pos_hash(seed) * 16.0 - 8.0;
            let z = camera.position.z
                + crate::client::particles::particle_pos_hash(seed + 1.0) * 16.0
                - 8.0;
            let y = camera.position.y
                + 6.0
                + crate::client::particles::particle_pos_hash(seed + 2.0) * 4.0;
            let bx = x.floor() as i32;
            let bz = z.floor() as i32;
            let blocked_above = ((camera.position.y.floor() as i32)..=y.ceil() as i32)
                .any(|by| self.world.get_block(bx, by, bz).is_solid());
            if blocked_above {
                continue;
            }
            self.particles.spawn(crate::client::particles::Particle {
                kind: crate::client::particles::ParticleKind::Rain,
                position: nalgebra::Point3::new(x, y, z),
                velocity: nalgebra::Vector3::new(0.0, -18.0, 0.0),
                age: 0.0,
                lifetime: 0.55,
                size: 0.08,
                color: [0.75, 0.85, 1.0, 0.25 + strength * 0.35],
                rotation: 0.0,
                texture_jitter: [0.0, 0.0],
                on_ground: false,
            });
        }
    }
}
