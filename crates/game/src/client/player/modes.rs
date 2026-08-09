use nalgebra::Vector3;

use super::math::*;
use super::state::Player;
use crate::client::keybind::{Action, InputState};
use crate::client::physics::*;
use crate::world::World;

impl Player {
    /// Creative mode flying — vanilla MC 1.8.9 EntityPlayer.moveEntityWithHeading when flying.
    /// XZ movement uses moveFlying with flySpeed as the acceleration factor.
    /// Y velocity is set in tick() before this call (vanilla: onLivingUpdate before super).
    pub(crate) fn tick_flying(
        &mut self,
        fwd: &Vector3<f32>,
        rgt: &Vector3<f32>,
        mf: f32,
        ms: f32,
        _input: &InputState,
    ) {
        // jumpMovementFactor = flySpeed * (sprinting ? 2 : 1)  (vanilla: EntityPlayer.moveEntityWithHeading)
        let fly_spd = self.fly_speed * if self.sprinting { 2.0 } else { 1.0 };

        // moveFlying adds acceleration to motionX/motionZ based on input direction
        self.move_flying(fwd, rgt, mf, ms, fly_spd);
        self.pre_move_drag_friction = AIR_FRICTION as f64;
        // Note: motionY was already set in tick() (vanilla: onLivingUpdate before super).
        // The damping (d3 * 0.6) and XZ friction (0.91) are applied in tick() after collision.
    }

    /// Walking/running physics — vanilla MC 1.8.9 EntityLivingBase.moveEntityWithHeading (ground branch).
    pub(crate) fn tick_walking(
        &mut self,
        fwd: &Vector3<f32>,
        rgt: &Vector3<f32>,
        mf: f32,
        ms: f32,
        input: &InputState,
        world: &World,
    ) {
        // MC 1.8.9 jump logic: uses held state (movementInput.jump) with a
        // 10-tick cooldown (jumpTicks). The else branch resets the cooldown
        // when the key is released so an immediate re-press can jump again.
        if input.is_held(Action::Jump) {
            if self.on_ground && self.jump_ticks == 0 {
                self.velocity.y = self.jump_upwards_motion();
                if self.sprinting {
                    // EntityLivingBase.jump uses yaw * 0.017453292F here,
                    // unlike moveFlying's yaw * PI / 180.0F expression.
                    let angle = self.camera.mc_yaw_degrees() * 0.017453292_f32;
                    self.velocity.x -= (vanilla_sin(angle) * SPRINT_JUMP_BOOST) as f64;
                    self.velocity.z += (vanilla_cos(angle) * SPRINT_JUMP_BOOST) as f64;
                }
                self.jump_ticks = 10;
            }
        } else {
            self.jump_ticks = 0;
        }

        let friction = if self.on_ground {
            self.block_slipperiness_below(world) * AIR_FRICTION
        } else {
            AIR_FRICTION
        };

        // Vanilla reads friction twice (pre- and post-moveFlying), both from the
        // pre-move position. Save it so apply_post_move_physics uses the same value.
        self.pre_move_drag_friction = friction as f64;

        // EntityPlayer overrides getAIMoveSpeed to read the current attribute,
        // so ground sprint changes apply this tick. Only jumpMovementFactor
        // (the air branch) is deferred by EntityPlayer.onLivingUpdate.
        let acceleration = if self.on_ground {
            self.ground_movement_speed()
                * (GROUND_FRICTION_FACTOR / (friction * friction * friction))
        } else {
            self.air_acceleration
        };

        self.move_flying(fwd, rgt, mf, ms, acceleration);
    }

    /// Swimming physics (when in water) — vanilla MC 1.8.9 EntityLivingBase.moveEntityWithHeading (water branch).
    pub(crate) fn tick_swimming(
        &mut self,
        fwd: &Vector3<f32>,
        rgt: &Vector3<f32>,
        mf: f32,
        ms: f32,
        input: &InputState,
        world: &World,
    ) {
        // EntityLivingBase.updateAITick is called when the jump key is held in
        // water. Sneak does not apply an artificial downward impulse in 1.8.9.
        if input.is_held(Action::Jump) {
            self.velocity.y += 0.03999999910593033;
        }

        // Water movement acceleration (vanilla: moveFlying(strafe, forward, 0.02F))
        // Drag is applied in apply_post_move_physics, NOT here (to avoid double-drag).
        let acceleration = 0.02;
        self.move_flying(fwd, rgt, mf, ms, acceleration);

        // Update oxygen
        if self.is_head_in_liquid(world) {
            self.oxygen -= 1;
            if self.oxygen <= 0 {
                self.oxygen_damage_timer += 1;
                if self.oxygen_damage_timer >= 20 {
                    self.oxygen_damage_timer = 0;
                }
            } else {
                self.oxygen_damage_timer = 0;
            }
        } else {
            self.oxygen = (self.oxygen + 5).min(300);
            self.oxygen_damage_timer = 0;
        }
    }

    /// EntityLivingBase.moveEntityWithHeading lava branch. It has the same
    /// moveFlying factor and jump impulse as water, but uses 0.5 drag.
    pub(crate) fn tick_lava(
        &mut self,
        fwd: &Vector3<f32>,
        rgt: &Vector3<f32>,
        mf: f32,
        ms: f32,
        input: &InputState,
    ) {
        if input.is_held(Action::Jump) {
            self.velocity.y += 0.03999999910593033;
        }
        self.move_flying(fwd, rgt, mf, ms, 0.02);
    }
}
