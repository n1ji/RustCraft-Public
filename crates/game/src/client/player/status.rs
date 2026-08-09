use nalgebra::{Point3, Vector3};

use super::camera::Camera;
use super::math::*;
use super::state::{MovementSpeedAttribute, Player};
use crate::client::physics::*;
use crate::world::World;

impl Player {
    /// Call each frame with tick interpolation factor (0.0-1.0) for smooth rendering.
    pub fn update_render_position(&mut self, alpha: f32, world: Option<&World>) {
        let alpha64 = alpha as f64;
        self.render_position = Point3::new(
            (self.prev_position.x + (self.position.x - self.prev_position.x) * alpha64) as f32,
            (self.prev_position.y + (self.position.y - self.prev_position.y) * alpha64) as f32,
            (self.prev_position.z + (self.position.z - self.prev_position.z) * alpha64) as f32,
        );
        self.render_chasing_position = Point3::new(
            (self.prev_chasing_position.x
                + (self.chasing_position.x - self.prev_chasing_position.x) * alpha64)
                as f32,
            (self.prev_chasing_position.y
                + (self.chasing_position.y - self.prev_chasing_position.y) * alpha64)
                as f32,
            (self.prev_chasing_position.z
                + (self.chasing_position.z - self.prev_chasing_position.z) * alpha64)
                as f32,
        );
        let eye = if self.sneaking {
            (PLAYER_EYE_HEIGHT - 0.08) as f32
        } else {
            PLAYER_EYE_HEIGHT as f32
        };
        let mut cam_pos = self.render_position + Vector3::new(0.0, eye, 0.0);

        self.camera.reverse_view = self.camera_mode == 2;

        // Third-person camera offset
        if self.camera_mode > 0 {
            let offset = self.camera.front * if self.camera_mode == 2 { 4.0 } else { -4.0 };
            if let Some(w) = world {
                // EntityRenderer.orientCamera checks eight rays around the eye
                // so the camera does not clip through a wall at screen corners.
                let mut allowed_distance = offset.norm();
                for i in 0..8 {
                    let corner = Vector3::new(
                        if i & 1 == 0 { -0.1 } else { 0.1 },
                        if i & 2 == 0 { -0.1 } else { 0.1 },
                        if i & 4 == 0 { -0.1 } else { 0.1 },
                    );
                    let start = cam_pos + corner;
                    let steps = 64;
                    let step = offset / steps as f32;
                    for step_index in 1..=steps {
                        let sample = start + step * step_index as f32;
                        if w.get_block(
                            sample.x.floor() as i32,
                            sample.y.floor() as i32,
                            sample.z.floor() as i32,
                        )
                        .is_solid()
                        {
                            let hit_distance =
                                offset.norm() * (step_index - 1) as f32 / steps as f32;
                            allowed_distance = allowed_distance.min(hit_distance);
                            break;
                        }
                    }
                }
                cam_pos += offset.normalize() * allowed_distance;
            } else {
                cam_pos += offset;
            }
        }

        self.camera.position = cam_pos;
        self.camera.view_bobbing = self.camera_mode == 0 && self.camera.view_bobbing;
        let delta_distance = self.distance_walked_modified - self.prev_distance_walked_modified;
        self.camera.bob_phase = -(self.distance_walked_modified + delta_distance * alpha);
        self.camera.bob_amount =
            self.prev_camera_yaw + (self.camera_yaw - self.prev_camera_yaw) * alpha;
        self.camera.bob_pitch =
            self.prev_camera_pitch + (self.camera_pitch - self.prev_camera_pitch) * alpha;
        self.camera.hurt_time = self.hurt_time;
        self.camera.prev_hurt_time = self.prev_hurt_time;
    }

    /// Starts visual feedback only after an authoritative damage/status packet.
    pub fn trigger_hurt(&mut self) {
        self.prev_hurt_time = 0.45;
        self.hurt_time = 0.45;
    }

    pub fn add_effect(&mut self, effect: crate::entity::EntityEffectState) {
        if let Some(existing) = self
            .active_effects
            .iter_mut()
            .find(|active| active.effect_id == effect.effect_id)
        {
            *existing = effect;
        } else {
            self.active_effects.push(effect);
        }
    }

    pub fn remove_effect(&mut self, effect_id: i8) {
        self.active_effects
            .retain(|effect| effect.effect_id != effect_id);
    }

    pub(crate) fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.sprinting_ticks_left = if sprinting { 600 } else { 0 };
    }

    pub fn take_pending_horse_jump(&mut self) -> Option<i32> {
        self.pending_horse_jump.take()
    }

    pub fn movement_sprinting(&self) -> bool {
        self.movement_sprinting
    }

    /// Successful local `attackTargetEntityWithCurrentItem`: sprinting adds one
    /// knockback level, and positive knockback damps horizontal motion by 0.6
    /// and cancels sprinting.
    pub fn on_attack_entity(&mut self, knockback_level: i16) {
        if self.sprinting || knockback_level > 0 {
            self.velocity.x *= 0.6;
            self.velocity.z *= 0.6;
            self.set_sprinting(false);
        }
    }

    /// Vanilla `EntityPlayer.handleStatusUpdate(9)` → `onItemUseFinish()`:
    /// the server sends S19 status 9 when it consumes the food.  The client
    /// must clear its local item-use state in response so movement returns to
    /// full speed on the next tick.
    pub fn on_item_use_finished(&mut self) {
        self.item_use_finished = true;
    }

    /// Returns true once after the server confirms food/potion consumption via
    /// S19 status 9.  The caller (frame loop) clears `item_use_active` and
    /// resets this flag.
    pub fn take_item_use_finished(&mut self) -> bool {
        let finished = self.item_use_finished;
        self.item_use_finished = false;
        finished
    }

    /// Vanilla `Entity.getEyeHeight` position: feet plus 1.62 (minus 0.08
    /// while sneaking). Unlike `camera.position` this is not displaced by
    /// third-person view or view bobbing.
    pub fn eye_position(&self) -> Point3<f64> {
        let eye_height = if self.sneaking {
            PLAYER_EYE_HEIGHT - 0.08
        } else {
            PLAYER_EYE_HEIGHT
        };
        Point3::new(
            self.position.x,
            self.position.y + eye_height,
            self.position.z,
        )
    }

    /// Read-only exposure for owned scripting/debug snapshots. The gameplay
    /// field remains private so callers cannot mutate collision state.
    pub fn in_lava(&self) -> bool {
        self.in_lava
    }

    /// Apply the local player's S20 attribute snapshot. Vanilla's
    /// NetHandlerPlayClient routes this packet to EntityPlayerSP as well as to
    /// every other living entity; the local player is not in our EntityManager.
    pub fn apply_entity_properties(&mut self, properties: Vec<crate::net::packet::EntityProperty>) {
        for property in properties {
            if property.key == "generic.movementSpeed" {
                self.movement_speed_attribute = MovementSpeedAttribute {
                    base: property.value,
                    modifiers: property.modifiers,
                };
            }
        }
    }
}
