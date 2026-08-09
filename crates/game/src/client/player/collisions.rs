use nalgebra::{Point3, Vector3};

use super::math::*;
use super::state::{MovementSpeedAttribute, Player};
use crate::client::physics::*;
use crate::entity::EntityManager;
use crate::world::World;

impl Player {
    pub(crate) fn ground_movement_speed(&self) -> f32 {
        let attribute = &self.movement_speed_attribute;
        let mut base = attribute.base;
        for modifier in attribute.modifiers.iter().filter(|modifier| {
            modifier.operation == 0 && !MovementSpeedAttribute::is_local_dynamic_modifier(modifier)
        }) {
            base += modifier.amount;
        }
        let mut value = base;
        for modifier in attribute.modifiers.iter().filter(|modifier| {
            modifier.operation == 1 && !MovementSpeedAttribute::is_local_dynamic_modifier(modifier)
        }) {
            value += base * modifier.amount;
        }
        for modifier in attribute.modifiers.iter().filter(|modifier| {
            modifier.operation == 2 && !MovementSpeedAttribute::is_local_dynamic_modifier(modifier)
        }) {
            value *= 1.0 + modifier.amount;
        }
        for effect in &self.active_effects {
            let amplifier = (effect.amplifier as i32 + 1) as f64;
            match effect.effect_id {
                1 => value *= 1.0 + 0.20000000298023224 * amplifier,
                2 => value *= 1.0 - 0.15000000596046448 * amplifier,
                _ => {}
            }
        }
        // Vanilla applies the sprint modifier through the attribute system
        // (operation 2, amount 0.30000001192092896D) in double precision,
        // then casts the final result to float.  Computing it separately in
        // f32 produces a slightly different bit pattern that accumulates over
        // ticks and causes GrimAC Simulation violations.
        if self.sprinting {
            value *= 1.0 + 0.30000001192092896;
        }
        value.clamp(0.0, 1024.0) as f32
    }

    pub(crate) fn jump_upwards_motion(&self) -> f64 {
        let mut motion = JUMP_VELOCITY as f64;
        if let Some(effect) = self
            .active_effects
            .iter()
            .find(|effect| effect.effect_id == 8)
        {
            let boost = (effect.amplifier as i32 + 1) as f32 * 0.1_f32;
            motion += boost as f64;
        }
        motion
    }

    pub(crate) fn block_below(&self, world: &World) -> crate::world::block::Block {
        let x = self.position.x.floor() as i32;
        let y = (self.position.y - 0.20000000298023224).floor() as i32;
        let z = self.position.z.floor() as i32;
        let block = world.get_block(x, y, z);
        if block == crate::world::block::Block::Air {
            let below = world.get_block(x, y - 1, z);
            if matches!(
                below,
                crate::world::block::Block::OakFence
                    | crate::world::block::Block::SpruceFence
                    | crate::world::block::Block::BirchFence
                    | crate::world::block::Block::JungleFence
                    | crate::world::block::Block::DarkOakFence
                    | crate::world::block::Block::AcaciaFence
                    | crate::world::block::Block::NetherBrickFence
                    | crate::world::block::Block::CobblestoneWall
                    | crate::world::block::Block::OakFenceGate
                    | crate::world::block::Block::SpruceFenceGate
                    | crate::world::block::Block::BirchFenceGate
                    | crate::world::block::Block::JungleFenceGate
                    | crate::world::block::Block::DarkOakFenceGate
                    | crate::world::block::Block::AcaciaFenceGate
            ) {
                return below;
            }
        }
        block
    }

    pub(crate) fn is_on_ladder(&self, world: &World) -> bool {
        matches!(
            world.get_block(
                self.position.x.floor() as i32,
                self.position.y.floor() as i32,
                self.position.z.floor() as i32,
            ),
            crate::world::block::Block::Ladder | crate::world::block::Block::Vine
        )
    }

    pub(crate) fn intersects_block(
        &self,
        world: &World,
        target: crate::world::block::Block,
    ) -> bool {
        let bb = Aabb::new(&self.position, PLAYER_WIDTH, PLAYER_HEIGHT);
        let min_x = (bb.min_x + 0.001).floor() as i32;
        let min_y = (bb.min_y + 0.001).floor() as i32;
        let min_z = (bb.min_z + 0.001).floor() as i32;
        let max_x = (bb.max_x - 0.001).floor() as i32;
        let max_y = (bb.max_y - 0.001).floor() as i32;
        let max_z = (bb.max_z - 0.001).floor() as i32;
        (min_x..=max_x).any(|x| {
            (min_y..=max_y).any(|y| (min_z..=max_z).any(|z| world.get_block(x, y, z) == target))
        })
    }

    pub(crate) fn boat_collision_boxes(
        &self,
        entities: &crate::entity::EntityManager,
        local_entity_id: Option<i32>,
    ) -> Vec<Aabb> {
        entities
            .entities
            .values()
            .filter(|entity| entity.entity_type == crate::entity::EntityType::Boat)
            .filter(|entity| Some(entity.entity_id) != self.vehicle_id)
            .filter(|entity| entity.vehicle_id != local_entity_id)
            .map(|entity| {
                let position = Point3::new(
                    entity.position.x as f64,
                    entity.position.y as f64,
                    entity.position.z as f64,
                );
                Aabb::new(&position, 1.5_f32 as f64, 0.6_f32 as f64)
            })
            .collect()
    }

    pub(crate) fn update_riding_position(&mut self, entities: &crate::entity::EntityManager) {
        let Some(vehicle_id) = self.vehicle_id else {
            return;
        };
        let Some(vehicle) = entities.get(vehicle_id) else {
            return;
        };

        let vehicle_position = vehicle.position;
        let (x, y, z) = match vehicle.entity_type {
            // EntityBoat.updateRiderPosition overrides the generic entity
            // placement with a 0.4 block yaw-relative seat offset.
            crate::entity::EntityType::Boat => {
                let yaw = (vehicle.yaw as f64).to_radians();
                (
                    vehicle_position.x as f64 + yaw.cos() * 0.4,
                    vehicle_position.y as f64 - 0.3 - 0.35,
                    vehicle_position.z as f64 + yaw.sin() * 0.4,
                )
            }
            // Entity.updateRiderPosition uses getMountedYOffset() + the
            // player's -0.35 yOffset. Minecarts override mounted offset to 0;
            // other currently represented vehicles use Entity's height * .75.
            crate::entity::EntityType::MinecartEmpty
            | crate::entity::EntityType::MinecartChest
            | crate::entity::EntityType::MinecartFurnace
            | crate::entity::EntityType::MinecartTNT
            | crate::entity::EntityType::MinecartHopper
            | crate::entity::EntityType::MinecartSpawner
            | crate::entity::EntityType::MinecartCommand => (
                vehicle_position.x as f64,
                vehicle_position.y as f64 - 0.35,
                vehicle_position.z as f64,
            ),
            _ => {
                let (_, height) = vehicle.entity_type.bounding_box();
                (
                    vehicle_position.x as f64,
                    vehicle_position.y as f64 + height as f64 * 0.75 - 0.35,
                    vehicle_position.z as f64,
                )
            }
        };
        self.position = Point3::new(x, y, z);
        self.velocity = Vector3::zeros();
        self.fall_start_y = None;
        self.fall_distance = 0.0;
    }

    pub(crate) fn handle_water_movement(&mut self, world: &World) -> bool {
        // Vanilla: expand(0, -0.4, 0).contract(0.001, 0.001, 0.001)
        // expand(-y) raises minY and lowers maxY (shrinks vertically inward);
        // contract shrinks all six faces inward.
        // Result: detects water touching the player's lower body (0.4–1.4 above feet).
        let hw = PLAYER_WIDTH / 2.0;
        let bb = Aabb {
            min_x: self.position.x - hw + 0.001,
            min_y: self.position.y + 0.4000000059604645 + 0.001,
            min_z: self.position.z - hw + 0.001,
            max_x: self.position.x + hw - 0.001,
            max_y: self.position.y + PLAYER_HEIGHT - 0.4000000059604645 - 0.001,
            max_z: self.position.z + hw - 0.001,
        };
        let min_x = (bb.min_x + 0.001).floor() as i32;
        let min_y = (bb.min_y + 0.001).floor() as i32;
        let min_z = (bb.min_z + 0.001).floor() as i32;
        let max_x = (bb.max_x - 0.001 + 1.0).floor() as i32;
        let max_y = (bb.max_y + 1.0).floor() as i32;
        let max_z = (bb.max_z - 0.001 + 1.0).floor() as i32;
        let mut flow = Vector3::zeros();
        let mut in_water = false;

        for x in min_x..max_x {
            for y in min_y..max_y {
                for z in min_z..max_z {
                    let block = world.get_block(x, y, z);
                    if !matches!(
                        block,
                        crate::world::block::Block::FlowingWater
                            | crate::world::block::Block::StillWater
                    ) {
                        continue;
                    }
                    // World.handleMaterialAcceleration only considers a
                    // liquid voxel when the scanned AABB's integer upper Y
                    // bound reaches that voxel's real fluid surface.  Treating
                    // every flowing-water voxel as full height made the client
                    // enter swimming physics too early at shallow edges.
                    let metadata = world.get_block_metadata(x, y, z) as i32;
                    let effective_level = if metadata >= 8 { 0 } else { metadata };
                    let liquid_height_percent = (effective_level + 1) as f64 / 9.0;
                    let liquid_surface = y as f64 + 1.0 - liquid_height_percent;
                    if (max_y as f64) < liquid_surface {
                        continue;
                    }
                    in_water = true;
                    flow += self.water_flow_vector(world, x, y, z, block);
                }
            }
        }

        if in_water && flow.magnitude_squared() > 0.0 {
            self.velocity += flow.normalize() * 0.014;
        }
        in_water
    }

    /// Applies the client-side entity movement from an extending piston.
    /// Vanilla performs this in TileEntityPiston before the authoritative block
    /// updates arrive, so it cannot wait for normal static-block collision.
    pub fn push_by_extending_piston(
        &mut self,
        world: &World,
        piston_x: i32,
        piston_y: i32,
        piston_z: i32,
        facing: u8,
    ) {
        let (dx, dy, dz) = match facing {
            0 => (0, -1, 0),
            1 => (0, 1, 0),
            2 => (0, 0, -1),
            3 => (0, 0, 1),
            4 => (-1, 0, 0),
            5 => (1, 0, 0),
            _ => return,
        };

        let player_bb = Aabb::new(&self.position, PLAYER_WIDTH, PLAYER_HEIGHT);
        let mut block_x = piston_x + dx;
        let mut block_y = piston_y + dy;
        let mut block_z = piston_z + dz;
        let mut pushed = false;
        let mut slime = false;

        // The head always sweeps through the first cell ahead of the base.
        // Consecutive occupied cells are the blocks carried by this extension.
        for index in 0..12 {
            let block = world.get_block(block_x, block_y, block_z);
            let overlaps = player_bb.overlaps_block(block_x, block_y, block_z)
                || player_bb.overlaps_block(block_x + dx, block_y + dy, block_z + dz);
            pushed |= overlaps;
            slime |= overlaps && block == crate::world::block::Block::SlimeBlock;

            if index == 11 || block == crate::world::block::Block::Air {
                break;
            }
            block_x += dx;
            block_y += dy;
            block_z += dz;
        }

        if !pushed {
            return;
        }

        if slime {
            self.velocity.x = dx as f64;
            self.velocity.y = dy as f64;
            self.velocity.z = dz as f64;
        } else {
            self.position.x += dx as f64;
            self.position.y += dy as f64;
            self.position.z += dz as f64;
        }
    }

    pub(crate) fn is_in_lava(&self, world: &World) -> bool {
        // Vanilla: expand(-0.1, -0.4, -0.1)
        // Shrinks laterally by 0.1 on each side and vertically inward
        // (raises minY by 0.4, lowers maxY by 0.4) — only the core body.
        let hw = PLAYER_WIDTH / 2.0;
        let bb = Aabb {
            min_x: self.position.x - hw + 0.10000000149011612,
            min_y: self.position.y + 0.4000000059604645,
            min_z: self.position.z - hw + 0.10000000149011612,
            max_x: self.position.x + hw - 0.10000000149011612,
            max_y: self.position.y + PLAYER_HEIGHT - 0.4000000059604645,
            max_z: self.position.z + hw - 0.10000000149011612,
        };
        self.aabb_contains_material(world, &bb, false)
    }

    pub(crate) fn water_flow_vector(
        &self,
        world: &World,
        x: i32,
        y: i32,
        z: i32,
        water: crate::world::block::Block,
    ) -> Vector3<f64> {
        let decay = |x, y, z| {
            if world.get_block(x, y, z) != water {
                -1
            } else {
                let level = world.get_block_metadata(x, y, z) as i32;
                if level >= 8 {
                    0
                } else {
                    level
                }
            }
        };
        let level = decay(x, y, z);
        let mut flow = Vector3::zeros();
        for (dx, dz) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
            let neighbor = decay(x + dx, y, z + dz);
            if neighbor < 0 {
                let neighbor_block = world.get_block(x + dx, y, z + dz);
                if !neighbor_block.is_solid() {
                    let below = decay(x + dx, y - 1, z + dz);
                    if below >= 0 {
                        let delta = below - (level - 8);
                        flow.x += dx as f64 * delta as f64;
                        flow.z += dz as f64 * delta as f64;
                    }
                }
            } else {
                let delta = neighbor - level;
                flow.x += dx as f64 * delta as f64;
                flow.z += dz as f64 * delta as f64;
            }
        }
        // Falling liquid adds a strong down component next to a solid face.
        if world.get_block_metadata(x, y, z) >= 8
            && [(0, -1), (0, 1), (-1, 0), (1, 0)]
                .into_iter()
                .any(|(dx, dz)| {
                    world.get_block(x + dx, y, z + dz).is_solid()
                        || world.get_block(x + dx, y + 1, z + dz).is_solid()
                })
        {
            if flow.magnitude_squared() > 0.0 {
                flow = flow.normalize();
            }
            flow.y -= 6.0;
        }
        flow
    }

    pub(crate) fn is_offset_position_in_liquid(
        &self,
        world: &World,
        x: f64,
        y: f64,
        z: f64,
    ) -> bool {
        let bb = Aabb::new(&self.position, PLAYER_WIDTH, PLAYER_HEIGHT).offset(x, y, z);
        // Despite the vanilla method name, this is the escape check used
        // after a horizontal water/lava collision: the offset AABB must be
        // clear of blocks and outside all liquid.
        get_colliding_boxes(&bb, world).is_empty() && !self.aabb_contains_liquid(world, &bb)
    }

    pub(crate) fn aabb_contains_liquid(&self, world: &World, bb: &Aabb) -> bool {
        self.aabb_contains_material(world, bb, true)
    }

    pub(crate) fn aabb_contains_material(
        &self,
        world: &World,
        bb: &Aabb,
        include_water: bool,
    ) -> bool {
        let min_x = bb.min_x.floor() as i32;
        let min_y = bb.min_y.floor() as i32;
        let min_z = bb.min_z.floor() as i32;
        let max_x = (bb.max_x + 1.0).floor() as i32;
        let max_y = (bb.max_y + 1.0).floor() as i32;
        let max_z = (bb.max_z + 1.0).floor() as i32;
        (min_x..max_x).any(|x| {
            (min_y..max_y).any(|y| {
                (min_z..max_z).any(|z| {
                    if include_water {
                        matches!(
                            world.get_block(x, y, z),
                            crate::world::block::Block::FlowingWater
                                | crate::world::block::Block::StillWater
                                | crate::world::block::Block::FlowingLava
                                | crate::world::block::Block::StillLava
                        )
                    } else {
                        matches!(
                            world.get_block(x, y, z),
                            crate::world::block::Block::FlowingLava
                                | crate::world::block::Block::StillLava
                        )
                    }
                })
            })
        })
    }

    /// Check if the player's eyes are submerged in liquid.
    pub(crate) fn is_head_in_liquid(&self, world: &World) -> bool {
        let x = self.position.x.floor() as i32;
        let y = (self.position.y + PLAYER_EYE_HEIGHT * 0.5).floor() as i32;
        let z = self.position.z.floor() as i32;
        world.get_block(x, y, z).is_liquid()
    }

    /// Check if the player's feet are in liquid.
    pub(crate) fn is_feet_in_liquid(&self, world: &World) -> bool {
        let x = self.position.x.floor() as i32;
        let y = self.position.y.floor() as i32;
        let z = self.position.z.floor() as i32;
        world.get_block(x, y, z).is_liquid()
    }

    pub fn update_aspect(&mut self, w: u32, h: u32) {
        self.camera.update_aspect(w, h);
    }
    pub fn process_mouse(&mut self, dx: f32, dy: f32, sensitivity: f32, invert_y: bool) {
        self.camera.process_mouse(dx, dy, sensitivity, invert_y);
    }
}
