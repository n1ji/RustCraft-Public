use nalgebra::{Point3, Vector3};

use super::camera::Camera;
use super::math::*;
use crate::client::physics::*;

/// The current server-owned `generic.movementSpeed` attribute snapshot.
///
/// NetHandlerPlayClient replaces the base and every modifier when it receives
/// S20. Keeping the raw snapshot lets the local player apply the same
/// operation-0/1/2 ordering as `ModifiableAttributeInstance`.
#[derive(Clone, Debug)]
pub(crate) struct MovementSpeedAttribute {
    pub(crate) base: f64,
    pub(crate) modifiers: Vec<crate::net::packet::EntityPropertyModifier>,
}

impl MovementSpeedAttribute {
    pub(crate) fn is_local_dynamic_modifier(
        modifier: &crate::net::packet::EntityPropertyModifier,
    ) -> bool {
        modifier.uuid.eq_ignore_ascii_case(SPEED_POTION_MODIFIER)
            || modifier.uuid.eq_ignore_ascii_case(SLOWNESS_POTION_MODIFIER)
            || modifier.uuid.eq_ignore_ascii_case(SPRINTING_SPEED_MODIFIER)
    }
}

/// MC 1.8.9 player — reads InputState, never raw keys.
pub struct Player {
    pub camera: Camera,
    /// Vanilla Entity.pos* and motion* are doubles. These are authoritative
    /// gameplay state; rendering receives an explicit f32 projection.
    pub position: Point3<f64>,
    pub velocity: Vector3<f64>,
    pub on_ground: bool,
    pub flying: bool,
    pub allow_flying: bool,
    /// Server-synchronized capability values used by EntityPlayer movement.
    pub fly_speed: f32,
    pub food_level: i32,
    pub sneaking: bool,
    pub sprinting: bool,
    /// Sprint state selected by EntityPlayerSP before this tick's movement.
    /// onUpdateWalkingPlayer reports this same current state through C0B.
    pub(crate) movement_sprinting: bool,
    /// Vanilla MovementInput values for C0C while riding. `move_strafe` uses
    /// protocol/vanilla sign convention: positive means left.
    pub move_strafe: f32,
    pub move_forward: f32,
    pub movement_jump: bool,
    /// Whether the player is using an item (eating, drinking, bow, blocking).
    pub using_item: bool,
    // For smooth rendering between ticks
    pub prev_position: Point3<f64>,
    pub render_position: Point3<f32>,
    pub chasing_position: Point3<f64>,
    pub prev_chasing_position: Point3<f64>,
    pub render_chasing_position: Point3<f32>,
    // MC 1.8.9 sprint/fly toggle timers (tick-based, not wall-clock)
    /// Counts down from 7 when the first tap of double-tap-W is detected.
    pub(crate) sprint_toggle_ticks: u32,
    /// EntityPlayerSP.sprintingTicksLeft. Vanilla refreshes this to 600 only
    /// when sprinting is enabled, then clears sprint at expiry.
    pub(crate) sprinting_ticks_left: u32,
    /// EntityPlayerSP horse-jump charge state.
    pub(crate) horse_jump_counter: i32,
    pub(crate) horse_jump_power: f32,
    pub(crate) pending_horse_jump: Option<i32>,
    /// Counts down from 7 when the first tap of double-tap-Space is detected.
    pub(crate) fly_toggle_ticks: u32,
    /// Whether jump was held at the start of the previous tick (for edge detection).
    pub(crate) prev_jump_held: bool,
    /// Whether forward was >= 0.8 at the end of the previous tick.
    pub(crate) prev_forward_fast: bool,
    /// Deferred air movement speed, updated after moveEntityWithHeading to
    /// match EntityPlayer.onLivingUpdate's jumpMovementFactor timing.
    pub(crate) air_acceleration: f32,
    // Fall damage tracking (server calculates actual damage)
    /// Y position when the player started falling (None = not falling)
    pub(crate) fall_start_y: Option<f64>,
    /// Distance fallen since last ground contact (for visual effects)
    pub fall_distance: f32,
    // Water/swimming state
    /// Whether the player is currently submerged in water
    pub in_water: bool,
    /// Entity.isInLava uses a contracted/expanded AABB separate from water.
    pub(crate) in_lava: bool,
    /// Oxygen level (300 = full, 0 = drowning)
    pub oxygen: i32,
    /// Ticks since last oxygen damage
    pub(crate) oxygen_damage_timer: i32,
    /// Whether the player collided horizontally this tick (vanilla: isCollidedHorizontally)
    pub collided_horizontally: bool,
    /// Vehicle attached by S1B; used to exclude the ridden boat from hard
    /// collision queries, matching World.getCollidingBoundingBoxes.
    pub vehicle_id: Option<i32>,
    /// Set by BlockWeb collision callbacks and consumed by the next moveEntity.
    pub(crate) in_web: bool,
    /// EntityLivingBase.jumpTicks — cooldown counter that prevents consecutive jumps.
    /// Vanilla decrements this every tick and only allows a jump when it reaches 0.
    pub(crate) jump_ticks: u32,
    /// Saved motionY before moveEntity for creative fly damping (vanilla d3 in moveEntityWithHeading)
    pub(crate) pre_move_motion_y: f64,
    /// Set by S19 status 9 (server-confirmed food consumption).  The frame
    /// loop polls this to clear `item_use_active` in sync with the server.
    pub(crate) item_use_finished: bool,
    /// Entity.moveEntityWithHeading reads friction twice: once before moveFlying for
    /// speed and once before the final drag multiply. Both reads use the block at the
    /// pre-move position; sampling the post-move block would diverge from vanilla when
    /// crossing a slipperiness boundary within a single tick.
    pub(crate) pre_move_drag_friction: f64,
    /// Camera mode: 0=First Person, 1=Third Person Back, 2=Third Person Front
    pub camera_mode: u8,
    /// Vanilla EntityLivingBase limb animation state.
    pub limb_swing: f32,
    pub limb_swing_amount: f32,
    /// Vanilla EntityPlayerSP first-person arm tracking.
    pub render_arm_yaw: f32,
    pub render_arm_pitch: f32,
    pub prev_render_arm_yaw: f32,
    pub prev_render_arm_pitch: f32,
    /// Vanilla renderYawOffset in protocol/MC degrees.
    pub body_yaw: f32,
    /// Time remaining for the server-authoritative hurt camera effect.
    pub hurt_time: f32,
    pub prev_hurt_time: f32,
    /// Vanilla Entity.distanceWalkedModified: total horizontal distance walked, used by setupViewBobbing.
    pub distance_walked_modified: f32,
    pub prev_distance_walked_modified: f32,
    /// Vanilla EntityPlayer.cameraYaw: horizontal speed factor [0, 1] for view bobbing amplitude.
    pub camera_yaw: f32,
    pub prev_camera_yaw: f32,
    /// Vanilla EntityLivingBase.cameraPitch: vertical motion factor [-1, 1] for view bobbing pitch.
    pub camera_pitch: f32,
    pub prev_camera_pitch: f32,
    /// Server-synchronized potion effects for the local player. The local
    /// player is intentionally not duplicated in EntityManager.
    pub active_effects: Vec<crate::entity::EntityEffectState>,
    pub(crate) movement_speed_attribute: MovementSpeedAttribute,
}

impl Player {
    pub fn new(position: Point3<f64>, aspect: f32) -> Self {
        Player {
            camera: Camera::new(
                Point3::new(
                    position.x as f32,
                    (position.y + PLAYER_EYE_HEIGHT) as f32,
                    position.z as f32,
                ),
                aspect,
            ),
            position,
            velocity: Vector3::zeros(),
            on_ground: false,
            flying: false,
            allow_flying: false,
            fly_speed: FLY_SPEED,
            food_level: 20,
            sneaking: false,
            sprinting: false,
            movement_sprinting: false,
            move_strafe: 0.0,
            move_forward: 0.0,
            movement_jump: false,
            using_item: false,
            prev_position: position,
            render_position: Point3::new(position.x as f32, position.y as f32, position.z as f32),
            chasing_position: position,
            prev_chasing_position: position,
            render_chasing_position: Point3::new(
                position.x as f32,
                position.y as f32,
                position.z as f32,
            ),
            sprint_toggle_ticks: 0,
            sprinting_ticks_left: 0,
            horse_jump_counter: 0,
            horse_jump_power: 0.0,
            pending_horse_jump: None,
            fly_toggle_ticks: 0,
            prev_jump_held: false,
            prev_forward_fast: false,
            air_acceleration: 0.02,
            fall_start_y: None,
            fall_distance: 0.0,
            in_water: false,
            in_lava: false,
            oxygen: 300,
            oxygen_damage_timer: 0,
            collided_horizontally: false,
            vehicle_id: None,
            in_web: false,
            jump_ticks: 0,
            pre_move_motion_y: 0.0,
            item_use_finished: false,
            pre_move_drag_friction: 0.91,
            camera_mode: 0,
            limb_swing: 0.0,
            limb_swing_amount: 0.0,
            render_arm_yaw: -180.0,
            render_arm_pitch: 30.0,
            prev_render_arm_yaw: -180.0,
            prev_render_arm_pitch: 30.0,
            body_yaw: -180.0,
            hurt_time: 0.0,
            prev_hurt_time: 0.0,
            distance_walked_modified: 0.0,
            prev_distance_walked_modified: 0.0,
            camera_yaw: 0.0,
            prev_camera_yaw: 0.0,
            camera_pitch: 0.0,
            prev_camera_pitch: 0.0,
            active_effects: Vec::new(),
            // EntityPlayer.applyEntityAttributes sets this exact Java float
            // value as the base attribute value.
            movement_speed_attribute: MovementSpeedAttribute {
                base: WALK_SPEED as f64,
                modifiers: Vec::new(),
            },
        }
    }
}
