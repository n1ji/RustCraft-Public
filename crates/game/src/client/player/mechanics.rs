use nalgebra::Vector3;

use super::math::*;
use super::state::Player;
use crate::client::physics::*;
use crate::world::World;

impl Player {
    /// Post-move gravity and friction — applied after move_with_collision.
    /// Matches vanilla MC 1.8.9 EntityLivingBase.moveEntityWithHeading post-moveEntity section.
    pub fn apply_post_move_physics(&mut self, world: &World) {
        if self.in_water {
            // Water physics (vanilla: EntityLivingBase.moveEntityWithHeading water branch)
            self.velocity.x *= 0.8_f32 as f64;
            self.velocity.y *= 0.8_f32 as f64;
            self.velocity.z *= 0.8_f32 as f64;
            self.velocity.y -= 0.02;
        } else if self.in_lava {
            self.velocity.x *= 0.5;
            self.velocity.y *= 0.5;
            self.velocity.z *= 0.5;
            self.velocity.y -= 0.02;
        } else {
            // Land/air physics (vanilla: EntityLivingBase.moveEntityWithHeading ground branch).
            // The drag friction must use the pre-move block slipperiness, matching the second
            // f4 read in vanilla's moveEntityWithHeading (which happens before moveEntity).
            self.velocity.y -= GRAVITY;
            self.velocity.y *= 0.9800000190734863;
            self.velocity.x *= self.pre_move_drag_friction;
            self.velocity.z *= self.pre_move_drag_friction;
        }
    }

    pub(crate) fn move_flying(
        &mut self,
        _fwd: &Vector3<f32>,
        _rgt: &Vector3<f32>,
        forward: f32,
        strafe_right: f32,
        friction: f32,
    ) {
        // Vanilla Entity.moveFlying: strafe is moveStrafe (positive = left),
        // forward is moveForward (positive = forward).  RustCraft's strafe_right
        // is positive-right, so we negate to get vanilla's strafe.
        let strafe = -strafe_right;
        let mut f = strafe * strafe + forward * forward;

        if f < 1.0e-4 {
            return;
        }

        f = f.sqrt();
        if f < 1.0 {
            f = 1.0;
        }

        f = friction / f;
        let strafe = strafe * f;
        let forward = forward * f;
        let (f1, f2) = vanilla_yaw_sin_cos(self.camera.mc_yaw_degrees());
        // motionX += strafe * f2 - forward * f1
        // motionZ += forward * f2 + strafe * f1
        self.velocity.x += (strafe * f2 - forward * f1) as f64;
        self.velocity.z += (forward * f2 + strafe * f1) as f64;
    }

    pub(crate) fn block_slipperiness_below(&self, world: &World) -> f32 {
        let x = self.position.x.floor() as i32;
        let y = (self.position.y - 1.0).floor() as i32;
        let z = self.position.z.floor() as i32;
        world.get_block(x, y, z).properties().slipperiness
    }

    /// EntityPlayerSP.pushOutOfBlocks. A position is open only when its block
    /// and the block directly above are both non-normal cubes; the nearest
    /// horizontal open neighbor replaces one motion component with +/-0.1.
    pub(crate) fn push_out_of_blocks(&mut self, world: &World, x: f64, y: f64, z: f64) {
        let block_x = x.floor() as i32;
        let block_y = y.floor() as i32;
        let block_z = z.floor() as i32;
        let is_open =
            |x, y, z| !world.is_normal_cube(x, y, z) && !world.is_normal_cube(x, y + 1, z);
        if is_open(block_x, block_y, block_z) {
            return;
        }

        let local_x = x - block_x as f64;
        let local_z = z - block_z as f64;
        let mut distance = 9999.0;
        let mut direction = None;
        if is_open(block_x - 1, block_y, block_z) && local_x < distance {
            distance = local_x;
            direction = Some(0);
        }
        if is_open(block_x + 1, block_y, block_z) && 1.0 - local_x < distance {
            distance = 1.0 - local_x;
            direction = Some(1);
        }
        if is_open(block_x, block_y, block_z - 1) && local_z < distance {
            distance = local_z;
            direction = Some(4);
        }
        if is_open(block_x, block_y, block_z + 1) && 1.0 - local_z < distance {
            direction = Some(5);
        }

        match direction {
            Some(0) => self.velocity.x = -0.1,
            Some(1) => self.velocity.x = 0.1,
            Some(4) => self.velocity.z = -0.1,
            Some(5) => self.velocity.z = 0.1,
            _ => {}
        }
    }
}
