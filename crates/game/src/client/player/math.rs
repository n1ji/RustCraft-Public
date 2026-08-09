use nalgebra::Vector3;

// MC 1.8.9 movement constants
pub(crate) const WALK_SPEED: f32 = 0.1;
pub(crate) const FLY_SPEED: f32 = 0.05;
pub(crate) const JUMP_VELOCITY: f32 = 0.42;
pub(crate) const SPRINT_JUMP_BOOST: f32 = 0.2;
pub(crate) const GRAVITY: f64 = 0.08;
pub(crate) const AIR_FRICTION: f32 = 0.91;
pub(crate) const AIR_ACCELERATION: f32 = 0.02;
pub(crate) const SPRINT_AIR_ACCELERATION: f32 = 0.026;
pub(crate) const GROUND_FRICTION_FACTOR: f32 = 0.16277136;

/// Vanilla Minecraft's sin/cos lookup table. MathHelper.sin and cos use a
/// 65536-entry table indexed by `(int)(value * 10430.378F) & 65535`, which
/// produces slightly different float values than std::sin/cos.  GrimAC
/// replays vanilla's exact arithmetic, so we must match bit-for-bit.
pub(crate) const SIN_TABLE_SIZE: usize = 65536;

pub(crate) fn sin_table() -> &'static [f32; SIN_TABLE_SIZE] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[f32; SIN_TABLE_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0.0f32; SIN_TABLE_SIZE];
        let mut i = 0;
        while i < SIN_TABLE_SIZE {
            table[i] = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
            i += 1;
        }
        table
    })
}

pub(crate) fn vanilla_sin(value: f32) -> f32 {
    let index = (value * 10430.378) as i32;
    sin_table()[(index as usize) & 65535]
}

pub(crate) fn vanilla_cos(value: f32) -> f32 {
    let index = (value * 10430.378 + 16384.0) as i32;
    sin_table()[(index as usize) & 65535]
}

pub(crate) fn vanilla_yaw_sin_cos(mc_yaw_degrees: f32) -> (f32, f32) {
    // Entity.moveFlying evaluates rotationYaw * (float)Math.PI / 180.0F in
    // this order. Derive it from the same yaw sent in C05/C06 so movement and
    // server prediction cannot choose adjacent LUT entries after a radian
    // round trip.
    let angle = mc_yaw_degrees * std::f32::consts::PI / 180.0_f32;
    (vanilla_sin(angle), vanilla_cos(angle))
}

/// Vanilla Entity.moveFlying derives horizontal acceleration from yaw alone.
/// Keeping this separate from Camera::front prevents vertical look pitch from
/// changing ground movement.
pub(crate) fn horizontal_movement_basis(mc_yaw_degrees: f32) -> (Vector3<f32>, Vector3<f32>) {
    let (sin_yaw, cos_yaw) = vanilla_yaw_sin_cos(mc_yaw_degrees);
    (
        Vector3::new(-sin_yaw, 0.0, cos_yaw),
        Vector3::new(-cos_yaw, 0.0, -sin_yaw),
    )
}

// EntityLivingBase's synthetic movement-speed modifiers.  S20 supplies the
// server-owned attribute snapshot, while effects and the sprint flag are
// maintained locally by the vanilla client entity.
pub(crate) const SPEED_POTION_MODIFIER: &str = "91AEAA56-376B-4498-935B-2F7F68070635";
pub(crate) const SLOWNESS_POTION_MODIFIER: &str = "7107DE5E-7CE8-4030-940E-514C1F160890";
pub(crate) const SPRINTING_SPEED_MODIFIER: &str = "662A6B8D-DA3E-4C1C-8813-96EA6097278D";

use crate::util::wrap_degrees;
