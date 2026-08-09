use nalgebra::{Point3, Vector3};

use super::math::*;
use super::state::Player;
use crate::client::keybind::{Action, InputState};
use crate::client::physics::*;
use crate::entity::EntityManager;
use crate::util::wrap_degrees;
use crate::world::World;

impl Player {
    /// Called once per tick (20 Hz). Reads from InputState.
    /// Follows MC 1.8.9 EntityPlayerSP.onLivingUpdate → EntityLivingBase.onLivingUpdate flow.
    pub fn tick(
        &mut self,
        input: &InputState,
        world: &World,
        entities: &crate::entity::EntityManager,
        local_entity_id: Option<i32>,
    ) {
        self.prev_position = self.position;

        // If the world has no chunk at the player position the physics tick
        // would let the player fall through the (not-yet-loaded) ground.  Wait
        // until the chunk arrives before applying gravity/collision.
        let player_chunk_x = (self.position.x / 16.0).floor() as i32;
        let player_chunk_z = (self.position.z / 16.0).floor() as i32;
        let has_chunk = world.chunks.contains_key(&(player_chunk_x, player_chunk_z));

        // Entity.updateRidden clears motion before its nested onUpdate call.
        // This matters on the mounting tick, when on-foot momentum would
        // otherwise feed one speculative collision/movement update.
        if self.vehicle_id.is_some() {
            self.velocity = Vector3::zeros();
        }
        self.prev_hurt_time = self.hurt_time;
        self.hurt_time = (self.hurt_time - 1.0 / 20.0).max(0.0);
        for effect in &mut self.active_effects {
            effect.duration -= 1;
        }
        self.active_effects.retain(|effect| effect.duration > 0);

        // --- Decrement timers (vanilla: top of onLivingUpdate) ---
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        if self.fly_toggle_ticks > 0 {
            self.fly_toggle_ticks -= 1;
        }
        if self.sprint_toggle_ticks > 0 {
            self.sprint_toggle_ticks -= 1;
        }
        if self.sprinting_ticks_left > 0 {
            self.sprinting_ticks_left -= 1;
            if self.sprinting_ticks_left == 0 {
                self.set_sprinting(false);
            }
        }

        // Entity.onEntityUpdate calls handleWaterMovement before living
        // movement. Its AABB is expanded 0.4 down, contracted by 0.001, and
        // water flow contributes a normalized 0.014 impulse.
        self.in_water = self.handle_water_movement(world);
        self.in_lava = self.is_in_lava(world);

        // EntityLivingBase.onLivingUpdate clears tiny motion components before
        // jump/input acceleration. This is an exact 0.005D deadband.
        if self.velocity.x.abs() < 0.005 {
            self.velocity.x = 0.0;
        }
        if self.velocity.y.abs() < 0.005 {
            self.velocity.y = 0.0;
        }
        if self.velocity.z.abs() < 0.005 {
            self.velocity.z = 0.0;
        }

        // --- Read input (vanilla: movementInput.updatePlayerMoveState) ---
        if input.is_just_pressed(Action::TogglePerspective) {
            self.camera_mode = (self.camera_mode + 1) % 3;
            // When switching to Third Person Front (2), we invert the camera view direction during render
            // but we'll handle the camera look vector properly if needed.
        }

        let prev_jump_held = self.prev_jump_held;
        let jump_held = input.is_held(Action::Jump);
        self.prev_jump_held = jump_held;

        // EntityLivingBase.onLivingUpdate jumpTicks else branch:
        // when the jump key is released, reset the cooldown immediately
        // so an immediate re-press can trigger a new jump while still on ground.
        if !jump_held {
            self.jump_ticks = 0;
        }

        self.sneaking = input.is_held(Action::Sneak);

        // Vanilla Entity.moveFlying rotates movement using rotationYaw only;
        // rotationPitch is deliberately ignored.  The old code projected the
        // 3D camera look vector onto XZ, which collapses to zero at ±90° pitch
        // and made forward/backward movement impossible when looking straight
        // up or down.
        let (fwd, rgt) = horizontal_movement_basis(self.camera.mc_yaw_degrees());

        let mut mf = 0.0f32;
        let mut ms = 0.0f32;
        if input.is_held(Action::Forward) {
            mf += 1.0;
        }
        if input.is_held(Action::Backward) {
            mf -= 1.0;
        }
        if input.is_held(Action::StrafeLeft) {
            ms -= 1.0;
        }
        if input.is_held(Action::StrafeRight) {
            ms += 1.0;
        }

        // Sneak reduces movement speed (vanilla: MovementInputFromOptions)
        if self.sneaking {
            mf *= 0.3;
            ms *= 0.3;
        }

        // Item use slows movement to 20% but vanilla does NOT cancel sprinting —
        // EntityPlayerSP.onLivingUpdate and EntityLivingBase.moveEntityWithHeading
        // only scale input, they leave isSprinting() alone. Cancelling it locally
        // sends a spurious C0B STOP_SPRINTING that desyncs the server simulation.
        if self.using_item && self.vehicle_id.is_none() {
            mf *= 0.2;
            ms *= 0.2;
            self.sprint_toggle_ticks = 0;
        }

        // EntityPlayerSP.onLivingUpdate probes each lower-body corner before
        // sprint and movement dispatch. Its override uses a deterministic
        // horizontal 0.1 impulse, unlike Entity.pushOutOfBlocks' random push.
        let push_x = PLAYER_WIDTH * 0.35;
        let push_y = self.position.y + 0.5;
        self.push_out_of_blocks(
            world,
            self.position.x - push_x,
            push_y,
            self.position.z + push_x,
        );
        self.push_out_of_blocks(
            world,
            self.position.x - push_x,
            push_y,
            self.position.z - push_x,
        );
        self.push_out_of_blocks(
            world,
            self.position.x + push_x,
            push_y,
            self.position.z - push_x,
        );
        self.push_out_of_blocks(
            world,
            self.position.x + push_x,
            push_y,
            self.position.z + push_x,
        );

        // EntityPlayerSP passes these MovementInput values to C0C while
        // riding. RustCraft's horizontal strafe convention is positive-right,
        // whereas MovementInput/C0C is positive-left.
        self.move_strafe = -ms;
        self.move_forward = mf;
        self.movement_jump = jump_held;

        let forward_fast = mf >= 0.8;
        let can_sprint = self.food_level > 6 || self.allow_flying;
        let blinded = self
            .active_effects
            .iter()
            .any(|effect| effect.effect_id == 15);

        // --- Sprint logic (vanilla: EntityPlayerSP.onLivingUpdate lines 799-821) ---

        // Auto-sprint from double-tap W: fires when forward transitions slow → fast
        if self.on_ground
            && !self.sneaking
            && !self.prev_forward_fast
            && forward_fast
            && !self.sprinting
            && can_sprint
            && !self.using_item
            && !blinded
        {
            if self.sprint_toggle_ticks == 0 && !input.is_held(Action::Sprint) {
                self.sprint_toggle_ticks = 7;
            } else if self.sprint_toggle_ticks > 0 {
                self.set_sprinting(true);
            }
        }

        // Sprint from holding sprint key + forward
        if !self.sprinting
            && forward_fast
            && can_sprint
            && !self.using_item
            && !blinded
            && input.is_held(Action::Sprint)
        {
            self.set_sprinting(true);
        }

        // Cancel sprint when forward becomes slow, or collided horizontally
        if self.sprinting && (!forward_fast || self.collided_horizontally || !can_sprint) {
            self.set_sprinting(false);
        }

        // Cancel sprint when sneaking
        if self.sneaking {
            self.set_sprinting(false);
        }

        self.prev_forward_fast = forward_fast;

        // --- Creative fly toggle (vanilla: timer-based double-tap space) ---
        if self.allow_flying && !prev_jump_held && jump_held {
            if self.fly_toggle_ticks == 0 {
                self.fly_toggle_ticks = 7;
            } else {
                self.flying = !self.flying;
                self.velocity.y = 0.0;
                self.fly_toggle_ticks = 0;
            }
        }

        // --- Flying vertical movement (vanilla: before super.onLivingUpdate) ---
        if self.flying {
            let fly_spd = self.fly_speed * if self.sprinting { 2.0 } else { 1.0 };
            if jump_held {
                self.velocity.y += (fly_spd * 3.0) as f64;
            }
            if self.sneaking {
                self.velocity.y -= (fly_spd * 3.0) as f64;
            }
            // Save pre-move motionY for damping after collision (vanilla: d3 in moveEntityWithHeading)
            self.pre_move_motion_y = self.velocity.y;
        }

        // EntityPlayerSP horse-jump charge/release state machine. It runs
        // before super.onLivingUpdate, and sends RIDING_JUMP on key release.
        let riding_saddled_horse = self
            .vehicle_id
            .and_then(|vehicle_id| entities.get(vehicle_id))
            .is_some_and(|vehicle| {
                vehicle.entity_type == crate::entity::EntityType::Horse
                    && vehicle.visual.horse_saddled
            });
        if riding_saddled_horse {
            if self.horse_jump_counter < 0 {
                self.horse_jump_counter += 1;
                if self.horse_jump_counter == 0 {
                    self.horse_jump_power = 0.0;
                }
            }
            if prev_jump_held && !jump_held {
                self.horse_jump_counter = -10;
                self.pending_horse_jump = Some((self.horse_jump_power * 100.0) as i32);
            } else if !prev_jump_held && jump_held {
                self.horse_jump_counter = 0;
                self.horse_jump_power = 0.0;
            } else if prev_jump_held {
                self.horse_jump_counter += 1;
                if self.horse_jump_counter < 10 {
                    self.horse_jump_power = self.horse_jump_counter as f32 * 0.1;
                } else {
                    self.horse_jump_power = 0.8 + 2.0 / (self.horse_jump_counter - 9) as f32 * 0.1;
                }
            }
        } else {
            self.horse_jump_power = 0.0;
        }

        // EntityLivingBase.onLivingUpdate damps movement inputs immediately
        // before moveEntityWithHeading. Sprint eligibility above intentionally
        // uses the undamped EntityPlayerSP input, matching vanilla ordering.
        mf *= 0.98;
        ms *= 0.98;

        if !has_chunk {
            return;
        }

        // EntityPlayerSP selects sprint before movement and
        // onUpdateWalkingPlayer reports that same current state afterward.
        self.movement_sprinting = self.sprinting;

        // --- Main movement dispatch ---
        if self.flying {
            self.tick_flying(&fwd, &rgt, mf, ms, input);
        } else if self.in_water {
            self.tick_swimming(&fwd, &rgt, mf, ms, input, world);
        } else if self.in_lava {
            self.tick_lava(&fwd, &rgt, mf, ms, input);
        } else {
            self.tick_walking(&fwd, &rgt, mf, ms, input, world);
        }

        let on_ladder = self.is_on_ladder(world);
        if on_ladder {
            self.velocity.x = self.velocity.x.clamp(-0.15, 0.15);
            self.velocity.z = self.velocity.z.clamp(-0.15, 0.15);
            self.fall_distance = 0.0;
            self.velocity.y = self.velocity.y.max(-0.15);
            if self.sneaking && self.velocity.y < 0.0 {
                self.velocity.y = 0.0;
            }
        }

        // --- Collision (vanilla: Entity.moveEntity) ---
        // Web slowdown applies to this tick's requested displacement, while
        // motion is cleared before collision and rebuilt by post-move gravity.
        let was_on_ground = self.on_ground;
        let water_start_y = self.position.y;
        let mut delta = self.velocity;
        if self.in_web {
            self.in_web = false;
            delta.x *= 0.25;
            delta.y *= 0.05000000074505806;
            delta.z *= 0.25;
            self.velocity = Vector3::zeros();
        }
        let entity_collision_boxes = self.boat_collision_boxes(entities, local_entity_id);
        self.collided_horizontally = move_with_collision(
            &mut self.position,
            &mut self.velocity,
            &delta,
            PLAYER_WIDTH,
            PLAYER_HEIGHT,
            world,
            &entity_collision_boxes,
            &mut self.on_ground,
            self.sneaking,
        );
        // if self.on_ground != was_on_ground {
        //     let below_x = self.position.x.floor() as i32;
        //     let below_y = (self.position.y - 0.20000000298023224).floor() as i32;
        //     let below_z = self.position.z.floor() as i32;
        //     log::debug!(
        //         target: "rustcraft::movement",
        //         "ground transition: {} -> {}, pos=({:.6},{:.6},{:.6}), requested=({:.6},{:.6},{:.6}), below=({below_x},{below_y},{below_z}), block_state={}",
        //         was_on_ground,
        //         self.on_ground,
        //         self.position.x,
        //         self.position.y,
        //         self.position.z,
        //         delta.x,
        //         delta.y,
        //         delta.z,
        //         world.get_block_state(below_x, below_y, below_z)
        //     );
        // }

        // Vanilla checks isOnLadder again after moveEntity, at the resulting
        // position, before applying the 0.2 climb impulse.
        if self.collided_horizontally && self.is_on_ladder(world) {
            self.velocity.y = 0.2;
        }

        let block_below = self.block_below(world);
        if delta.y < 0.0
            && self.on_ground
            && block_below == crate::world::block::Block::SlimeBlock
            && !self.sneaking
        {
            self.velocity.y = -delta.y;
        }
        // BlockSoulSand.onEntityCollidedWithBlock is invoked by
        // Entity.doBlockCollisions for every Soul Sand voxel overlapped by the
        // entity AABB; it is not limited to the grounded block below.
        if self.intersects_block(world, crate::world::block::Block::SoulSand) {
            self.velocity.x *= 0.4;
            self.velocity.z *= 0.4;
        }
        if self.on_ground
            && block_below == crate::world::block::Block::SlimeBlock
            && !self.sneaking
            && self.velocity.y.abs() < 0.1
        {
            let factor = 0.4 + self.velocity.y.abs() * 0.2;
            self.velocity.x *= factor;
            self.velocity.z *= factor;
        }
        self.in_web = self.intersects_block(world, crate::world::block::Block::Cobweb);

        // EntityPlayer.onUpdate: smooth the cape anchor independently of the body.
        self.prev_chasing_position = self.chasing_position;
        let chasing_delta = self.position - self.chasing_position;
        for axis in 0..3 {
            if chasing_delta[axis].abs() > 10.0 {
                self.chasing_position[axis] = self.position[axis];
                self.prev_chasing_position[axis] = self.position[axis];
            }
        }
        self.chasing_position += (self.position - self.chasing_position) * 0.25;

        // EntityLivingBase: update limb swing from actual distance travelled.
        let dx = self.position.x - self.prev_position.x;
        let dz = self.position.z - self.prev_position.z;
        let distance_sq = dx * dx + dz * dz;
        // EntityLivingBase.onUpdate: f2 = sqrt(dx² + dz²) * 3.0F.
        // Keeping this exact is important because limbSwingAmount then feeds
        // both the stride amplitude and phase in ModelBiped.
        let target_limb_amount = (distance_sq.sqrt() * 3.0).min(1.0) as f32;
        self.limb_swing_amount += (target_limb_amount - self.limb_swing_amount) * 0.4;
        self.limb_swing += self.limb_swing_amount;

        // Entity.onUpdate: accumulate horizontal distance walked.
        // Used by EntityRenderer.setupViewBobbing to determine the bobbing phase.
        self.prev_distance_walked_modified = self.distance_walked_modified;
        self.distance_walked_modified += (distance_sq.sqrt() * 0.6) as f32;

        // EntityPlayer.onLivingUpdate: cameraYaw from horizontal speed.
        // cameraYaw [0, 1] controls the amplitude (f2) in setupViewBobbing.
        self.prev_camera_yaw = self.camera_yaw;
        let horiz_speed =
            (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z).sqrt() as f32;
        let mut camera_yaw_target = horiz_speed;
        if camera_yaw_target > 0.1 {
            camera_yaw_target = 0.1;
        }
        if !self.on_ground {
            camera_yaw_target = 0.0;
        }
        self.camera_yaw += (camera_yaw_target - self.camera_yaw) * 0.4;

        // EntityLivingBase.onLivingUpdate: cameraPitch from vertical motion.
        // cameraPitch [-1, 1] directly feeds the last rotation in setupViewBobbing.
        self.prev_camera_pitch = self.camera_pitch;
        let camera_pitch_target = (-self.velocity.y as f32 * 0.20000000298023224).atan() * 15.0;
        let camera_pitch_target = if self.on_ground {
            0.0
        } else {
            camera_pitch_target
        };
        self.camera_pitch += (camera_pitch_target - self.camera_pitch) * 0.8;

        // EntityLivingBase.updateDistance: body follows movement while the head
        // may turn independently, clamped to the vanilla +/-75 degree range.
        let head_yaw = self.camera.mc_yaw_degrees();
        let mut desired_body_yaw = self.body_yaw;
        if distance_sq > 0.0025000002 {
            desired_body_yaw = (dz.atan2(dx).to_degrees() - 90.0) as f32;
        }
        self.body_yaw += wrap_degrees(desired_body_yaw - self.body_yaw) * 0.3;
        let mut head_delta = wrap_degrees(head_yaw - self.body_yaw).clamp(-75.0, 75.0);
        self.body_yaw = head_yaw - head_delta;
        if head_delta * head_delta > 2500.0 {
            self.body_yaw += head_delta * 0.2;
            head_delta = wrap_degrees(head_yaw - self.body_yaw);
            self.body_yaw = head_yaw - head_delta;
        }

        self.prev_render_arm_yaw = self.render_arm_yaw;
        self.prev_render_arm_pitch = self.render_arm_pitch;
        self.render_arm_yaw += wrap_degrees(head_yaw - self.render_arm_yaw) * 0.5;
        self.render_arm_pitch +=
            wrap_degrees(self.camera.mc_pitch_degrees() - self.render_arm_pitch) * 0.5;

        // --- Post-move physics ---
        if self.flying {
            self.velocity.y = self.pre_move_motion_y * 0.6;
            self.velocity.x *= AIR_FRICTION as f64;
            self.velocity.z *= AIR_FRICTION as f64;
        } else {
            self.apply_post_move_physics(world);
        }

        // EntityLivingBase.moveEntityWithHeading performs the water/lava
        // escape check after drag and gravity. The resulting 0.3 impulse is
        // therefore not damped again before the next movement tick.
        if !self.flying
            && (self.in_water || self.in_lava)
            && self.collided_horizontally
            && self.is_offset_position_in_liquid(
                world,
                self.velocity.x,
                self.velocity.y + 0.6000000238418579 - self.position.y + water_start_y,
                self.velocity.z,
            )
        {
            self.velocity.y = 0.30000001192092896;
        }

        // EntityPlayer.onLivingUpdate updates jumpMovementFactor after
        // super.onLivingUpdate(), so sprint changes affect air acceleration on
        // the next tick. EntityPlayer.getAIMoveSpeed is different: its override
        // reads the current movement-speed attribute directly.
        let speed_in_air = 0.02f32;
        self.air_acceleration = speed_in_air;
        if self.sprinting {
            self.air_acceleration = (speed_in_air as f64 + speed_in_air as f64 * 0.3) as f32;
        }
        // EntityPlayerSP disables ordinary creative flight after the superclass
        // has completed this tick's flying movement and damping.
        if self.on_ground && self.flying {
            self.flying = false;
        }

        // --- Fall tracking ---
        if self.in_water {
            // Entity.handleWaterMovement clears fallDistance before the living
            // movement branch runs.
            self.fall_start_y = None;
            self.fall_distance = 0.0;
        } else if self.on_ground {
            if let Some(start_y) = self.fall_start_y {
                self.fall_distance = (start_y - self.position.y).max(0.0) as f32;
                self.fall_start_y = None;
            } else {
                self.fall_distance = 0.0;
            }
        } else if !self.flying && self.velocity.y < 0.0 {
            if self.fall_start_y.is_none() {
                self.fall_start_y = Some(self.position.y);
            }
        } else {
            self.fall_start_y = None;
            self.fall_distance = 0.0;
        }

        // Entity.updateRidden runs the rider's normal update with zeroed
        // motion, then lets the vehicle place the rider. The remote vehicle is
        // authoritative in this client, so mirror its final placement after
        // the local update instead of leaving the player at the speculative
        // on-foot collision position.
        self.update_riding_position(entities);

        // --- Sync camera with eye height ---
        // Done dynamically in update_render_position now to include third person offsets,
        // but we sync it here as a baseline for physics.
        let eye = if self.sneaking {
            (PLAYER_EYE_HEIGHT - 0.08) as f32
        } else {
            PLAYER_EYE_HEIGHT as f32
        };
        self.camera.position = Point3::new(
            self.position.x as f32,
            self.position.y as f32 + eye,
            self.position.z as f32,
        );
    }
}
