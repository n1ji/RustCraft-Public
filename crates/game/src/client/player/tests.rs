use super::*;
use crate::client::keybind::{Action, InputState};
use crate::world::World;
use crate::world::{block::Block, chunk::Chunk};
use nalgebra::{Matrix4, Point3, Vector3};

fn test_world(block: Block, metadata: u8) -> World {
    let mut world = World::new();
    world.chunks.insert((0, 0), Chunk::new(0, 0).into());
    world.set_block_state(0, 0, 0, (block.to_id() << 4) | metadata as u16);
    world
}

#[test]
fn horizontal_movement_basis_ignores_look_pitch() {
    // Looking straight up/down changes Camera::front's XZ length, but
    // vanilla moveFlying still produces the same yaw-based movement.
    // Uses MathHelper's lookup table, so tolerance is the table's ULP.
    let (forward, right) = horizontal_movement_basis(-180.0);
    assert!((forward.z + 1.0).abs() < 0.001, "forward.z = {}", forward.z);
    assert!(forward.x.abs() < 0.001, "forward.x = {}", forward.x);
    assert!((right.x - 1.0).abs() < 0.001, "right.x = {}", right.x);
    assert!(right.z.abs() < 0.001, "right.z = {}", right.z);
}

#[test]
fn water_and_lava_use_vanilla_aabb_material_checks() {
    let mut player = Player::new(Point3::new(0.5, 0.0, 0.5), 1.0);
    let water = test_world(Block::StillWater, 0);
    assert!(player.handle_water_movement(&water));
    assert!(!player.is_in_lava(&water));

    let lava = test_world(Block::StillLava, 0);
    assert!(!player.handle_water_movement(&lava));
    assert!(player.is_in_lava(&lava));
}

#[test]
fn water_escape_check_requires_an_open_non_liquid_offset() {
    let mut world = test_world(Block::Air, 0);
    let player = Player::new(Point3::new(0.5, 0.0, 0.5), 1.0);

    // The offset AABB is clear and outside liquid, so vanilla applies the
    // upward escape impulse after a horizontal water collision.
    assert!(player.is_offset_position_in_liquid(&world, 1.0, 0.0, 0.0));

    world.set_block(1, 0, 0, Block::StillWater);
    assert!(!player.is_offset_position_in_liquid(&world, 1.0, 0.0, 0.0));

    world.set_block(1, 0, 0, Block::Stone);
    assert!(!player.is_offset_position_in_liquid(&world, 1.0, 0.0, 0.0));
}

#[test]
fn swimming_keeps_vertical_motion_without_a_non_vanilla_clamp() {
    let world = test_world(Block::StillWater, 0);
    let mut player = Player::new(Point3::new(0.5, 0.0, 0.5), 1.0);
    player.velocity.y = 0.42;
    let (forward_basis, right_basis) = horizontal_movement_basis(player.camera.mc_yaw_degrees());
    let input = InputState::new();

    player.tick_swimming(&forward_basis, &right_basis, 0.0, 0.0, &input, &world);

    assert_eq!(player.velocity.y, 0.42);
}

#[test]
fn extending_piston_moves_players_and_slime_sets_piston_velocity() {
    let world = test_world(Block::Air, 0);
    let mut player = Player::new(Point3::new(0.5, 1.0, 0.5), 1.0);
    player.push_by_extending_piston(&world, 0, 0, 0, 1);
    assert_eq!(player.position.y, 2.0);

    let slime = test_world(Block::SlimeBlock, 0);
    let mut player = Player::new(Point3::new(0.5, 1.0, 0.5), 1.0);
    player.push_by_extending_piston(&slime, 0, -1, 0, 1);
    assert_eq!(player.velocity.y, 1.0);
}

#[test]
fn movement_potions_apply_vanilla_attribute_operations() {
    let mut player = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    player.active_effects = vec![
        crate::entity::EntityEffectState {
            effect_id: 1,
            amplifier: 1,
            duration: 100,
            hide_particles: false,
        },
        crate::entity::EntityEffectState {
            effect_id: 2,
            amplifier: 0,
            duration: 100,
            hide_particles: false,
        },
    ];
    let expected = ((WALK_SPEED as f64 * (1.0 + 0.20000000298023224 * 2.0))
        * (1.0 - 0.15000000596046448)) as f32;
    assert_eq!(player.ground_movement_speed(), expected);

    player.sprinting = true;
    assert_eq!(
        player.ground_movement_speed(),
        ((WALK_SPEED as f64 * (1.0 + 0.20000000298023224 * 2.0))
            * (1.0 - 0.15000000596046448)
            * (1.0 + 0.30000001192092896)) as f32
    );
}

#[test]
fn ground_sprint_attribute_applies_on_the_transition_tick() {
    let world = test_world(Block::Stone, 0);
    let mut player = Player::new(Point3::new(0.5, 1.0, 0.5), 1.0);
    player.on_ground = true;
    let (forward_basis, right_basis) = horizontal_movement_basis(player.camera.mc_yaw_degrees());
    let input = InputState::new();

    let horizontal_acceleration = |player: &mut Player| {
        player.velocity = Vector3::zeros();
        player.tick_walking(&forward_basis, &right_basis, 0.98, 0.0, &input, &world);
        (player.velocity.x * player.velocity.x + player.velocity.z * player.velocity.z).sqrt()
    };

    player.set_sprinting(false);
    let walking = horizontal_acceleration(&mut player);
    player.set_sprinting(true);
    let sprinting = horizontal_acceleration(&mut player);
    player.set_sprinting(false);
    let walking_after_stop = horizontal_acceleration(&mut player);

    let expected_ratio = (1.0 + 0.30000001192092896) as f32 as f64;
    assert!((sprinting / walking - expected_ratio).abs() < 1.0e-6);
    assert_eq!(walking_after_stop, walking);
}

#[test]
fn sprinting_uses_vanilla_six_hundred_tick_lifetime() {
    let mut player = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    player.set_sprinting(true);
    assert!(player.sprinting);
    assert_eq!(player.sprinting_ticks_left, 600);
    player.set_sprinting(false);
    assert!(!player.sprinting);
    assert_eq!(player.sprinting_ticks_left, 0);
}

#[test]
fn successful_attack_applies_vanilla_knockback_slowdown() {
    let mut sprinting = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    sprinting.velocity = Vector3::new(0.5, 0.25, -0.25);
    sprinting.set_sprinting(true);
    sprinting.on_attack_entity(0);
    assert_eq!(sprinting.velocity, Vector3::new(0.3, 0.25, -0.15));
    assert!(!sprinting.sprinting);

    let mut enchanted = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    enchanted.velocity = Vector3::new(0.5, 0.25, -0.25);
    enchanted.on_attack_entity(1);
    assert_eq!(enchanted.velocity, Vector3::new(0.3, 0.25, -0.15));

    let mut ordinary = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    ordinary.velocity = Vector3::new(0.5, 0.25, -0.25);
    ordinary.on_attack_entity(0);
    assert_eq!(ordinary.velocity, Vector3::new(0.5, 0.25, -0.25));
}

#[test]
fn saddled_horse_jump_sends_vanilla_release_charge() {
    let world = test_world(Block::Air, 0);
    let mut entities = crate::entity::EntityManager::new();
    let mut horse = crate::entity::Entity::new(
        7,
        crate::entity::EntityType::Horse,
        Point3::new(0.0, 0.0, 0.0),
    );
    horse.visual.horse_saddled = true;
    entities.spawn(horse);
    let mut player = Player::new(Point3::new(0.0, 2.0, 0.0), 1.0);
    player.vehicle_id = Some(7);
    let mut input = InputState::new();
    input.on_key_down(Action::Jump);
    player.tick(&input, &world, &entities, Some(1));
    input.tick_reset();
    player.tick(&input, &world, &entities, Some(1));
    input.on_key_up(Action::Jump);
    player.tick(&input, &world, &entities, Some(1));
    assert_eq!(player.take_pending_horse_jump(), Some(10));
}

#[test]
fn entity_properties_replace_local_movement_speed_snapshot() {
    let mut player = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    player.sprinting = true;
    player.add_effect(crate::entity::EntityEffectState {
        effect_id: 1,
        amplifier: 0,
        duration: 100,
        hide_particles: false,
    });
    player.apply_entity_properties(vec![crate::net::packet::EntityProperty {
        key: "generic.movementSpeed".to_string(),
        value: 0.2,
        modifiers: vec![
            crate::net::packet::EntityPropertyModifier {
                uuid: "custom-add".to_string(),
                amount: 0.1,
                operation: 0,
            },
            crate::net::packet::EntityPropertyModifier {
                uuid: "custom-base-scale".to_string(),
                amount: 0.5,
                operation: 1,
            },
            // A snapshot may already contain a local potion/sprint UUID.
            // Vanilla replaces that modifier rather than applying it twice.
            crate::net::packet::EntityPropertyModifier {
                uuid: SPEED_POTION_MODIFIER.to_string(),
                amount: 0.2,
                operation: 2,
            },
            crate::net::packet::EntityPropertyModifier {
                uuid: SPRINTING_SPEED_MODIFIER.to_ascii_lowercase(),
                amount: 0.3,
                operation: 2,
            },
        ],
    }]);

    // ModifiableAttributeInstance: ((0.2 + 0.1) + 0.3 * 0.5) * 1.2 * 1.3.
    assert_eq!(
        player.ground_movement_speed(),
        (0.45_f64 * 1.2 * 1.3) as f32
    );
}

#[test]
fn riding_position_uses_vanilla_boat_and_minecart_offsets() {
    let mut player = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    let mut entities = crate::entity::EntityManager::new();
    let mut boat = crate::entity::Entity::new(
        7,
        crate::entity::EntityType::Boat,
        Point3::new(10.0, 20.0, 30.0),
    );
    boat.yaw = 90.0;
    entities.spawn(boat);
    player.vehicle_id = Some(7);
    player.update_riding_position(&entities);
    assert_eq!(
        player.position,
        Point3::new(10.0, 20.0_f32 as f64 - 0.3 - 0.35, 30.4)
    );

    let minecart = crate::entity::Entity::new(
        8,
        crate::entity::EntityType::MinecartHopper,
        Point3::new(-4.0, 12.0, 3.0),
    );
    entities.spawn(minecart);
    player.vehicle_id = Some(8);
    player.update_riding_position(&entities);
    assert_eq!(player.position, Point3::new(-4.0, 11.65, 3.0));
}

#[test]
fn player_push_out_uses_nearest_horizontal_open_block() {
    let world = test_world(Block::Stone, 0);
    let mut player = Player::new(Point3::new(0.0, 0.0, 0.0), 1.0);
    player.push_out_of_blocks(&world, 0.21, 0.5, 0.21);
    // West wins the equal-distance tie because vanilla checks it first.
    assert_eq!(player.velocity.x, -0.1);
    assert_eq!(player.velocity.z, 0.0);
}

#[test]
fn player_push_out_does_not_treat_slab_as_normal_cube() {
    let world = test_world(Block::StoneSlab, 0);
    let mut player = Player::new(Point3::new(0.0, 0.0, 0.0), 1.0);
    player.push_out_of_blocks(&world, 0.21, 0.5, 0.21);
    assert_eq!(player.velocity, Vector3::zeros());
}

#[test]
fn player_push_out_does_not_treat_redstone_block_as_normal_cube() {
    let world = test_world(Block::RedstoneBlock, 0);
    let mut player = Player::new(Point3::new(0.0, 0.0, 0.0), 1.0);
    player.push_out_of_blocks(&world, 0.21, 0.5, 0.21);
    assert_eq!(player.velocity, Vector3::zeros());
}

#[test]
fn soul_sand_collision_detects_airborne_aabb_overlap() {
    let world = test_world(Block::SoulSand, 0);
    let player = Player::new(Point3::new(0.5, 0.5, 0.5), 1.0);
    assert!(player.intersects_block(&world, Block::SoulSand));
}

#[test]
fn jump_boost_uses_float_increment_promoted_to_double() {
    let mut player = Player::new(Point3::new(0.0, 64.0, 0.0), 1.0);
    player
        .active_effects
        .push(crate::entity::EntityEffectState {
            effect_id: 8,
            amplifier: 2,
            duration: 100,
            hide_particles: false,
        });
    assert_eq!(
        player.jump_upwards_motion(),
        JUMP_VELOCITY as f64 + (3.0_f32 * 0.1_f32) as f64
    );
}

#[test]
fn test_frustum_signs() {
    let f = 1.0 / (80.0f32.to_radians() / 2.0).tan();
    let n = 0.1;
    let fa = 100.0;
    let aspect = 1.77;

    let proj = Matrix4::new(
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        -f,
        0.0,
        0.0,
        0.0,
        0.0,
        fa / (n - fa),
        fa * n / (n - fa),
        0.0,
        0.0,
        -1.0,
        0.0,
    );

    let pos = Point3::new(10.0, 20.0, 30.0);

    for yaw_deg in (-180..=180).step_by(30) {
        for pitch_deg in (-60..=60).step_by(30) {
            let yaw = (yaw_deg as f32).to_radians();
            let pitch = (pitch_deg as f32).to_radians();

            let front = Vector3::new(
                yaw.cos() * pitch.cos(),
                pitch.sin(),
                yaw.sin() * pitch.cos(),
            )
            .normalize();
            let right = front.cross(&Vector3::new(0.0, 1.0, 0.0)).normalize();
            let up = right.cross(&front).normalize();

            let view = Matrix4::look_at_rh(&pos, &(pos + front), &up);
            let vp = proj * view;
            let frustum = Frustum::from_view_proj(&vp);

            // 1. Point in center front: should be inside all planes
            let pt_center = pos + front * 10.0;
            let pt_center_arr = [pt_center.x, pt_center.y, pt_center.z];
            for (idx, plane) in frustum.planes.iter().enumerate() {
                let val = plane[0] * pt_center_arr[0]
                    + plane[1] * pt_center_arr[1]
                    + plane[2] * pt_center_arr[2]
                    + plane[3];
                assert!(
                    val >= -0.01,
                    "Center point culled! yaw: {}, pitch: {}, plane: {}, val: {}",
                    yaw_deg,
                    pitch_deg,
                    idx,
                    val
                );
            }

            // 2. Point far left (off-screen left): should be culled by the left-right plane
            let pt_left = pos + front * 10.0 - right * 15.0;
            let pt_left_arr = [pt_left.x, pt_left.y, pt_left.z];
            let mut left_inside = true;
            for plane in &frustum.planes {
                let val = plane[0] * pt_left_arr[0]
                    + plane[1] * pt_left_arr[1]
                    + plane[2] * pt_left_arr[2]
                    + plane[3];
                if val < 0.0 {
                    left_inside = false;
                    break;
                }
            }
            assert!(
                !left_inside,
                "Off-screen left point not culled! yaw: {}, pitch: {}",
                yaw_deg, pitch_deg
            );

            // 3. Point far right (off-screen right): should be culled
            let pt_right = pos + front * 10.0 + right * 15.0;
            let pt_right_arr = [pt_right.x, pt_right.y, pt_right.z];
            let mut right_inside = true;
            for plane in &frustum.planes {
                let val = plane[0] * pt_right_arr[0]
                    + plane[1] * pt_right_arr[1]
                    + plane[2] * pt_right_arr[2]
                    + plane[3];
                if val < 0.0 {
                    right_inside = false;
                    break;
                }
            }
            assert!(
                !right_inside,
                "Off-screen right point not culled! yaw: {}, pitch: {}",
                yaw_deg, pitch_deg
            );

            // 4. Point far up (off-screen top): should be culled
            let pt_up = pos + front * 10.0 + up * 15.0;
            let pt_up_arr = [pt_up.x, pt_up.y, pt_up.z];
            let mut up_inside = true;
            for plane in &frustum.planes {
                let val = plane[0] * pt_up_arr[0]
                    + plane[1] * pt_up_arr[1]
                    + plane[2] * pt_up_arr[2]
                    + plane[3];
                if val < 0.0 {
                    up_inside = false;
                    break;
                }
            }
            assert!(
                !up_inside,
                "Off-screen top point not culled! yaw: {}, pitch: {}",
                yaw_deg, pitch_deg
            );

            // 5. Point far down (off-screen bottom): should be culled
            let pt_down = pos + front * 10.0 - up * 15.0;
            let pt_down_arr = [pt_down.x, pt_down.y, pt_down.z];
            let mut down_inside = true;
            for plane in &frustum.planes {
                let val = plane[0] * pt_down_arr[0]
                    + plane[1] * pt_down_arr[1]
                    + plane[2] * pt_down_arr[2]
                    + plane[3];
                if val < 0.0 {
                    down_inside = false;
                    break;
                }
            }
            assert!(
                !down_inside,
                "Off-screen bottom point not culled! yaw: {}, pitch: {}",
                yaw_deg, pitch_deg
            );
        }
    }
}

#[test]
fn projection_uses_vulkan_zero_to_one_depth_range() {
    let camera = Camera::new(Point3::origin(), 16.0 / 9.0);
    let projection = camera.projection_matrix();
    let near = projection * nalgebra::Vector4::new(0.0, 0.0, -camera.near, 1.0);
    let far = projection * nalgebra::Vector4::new(0.0, 0.0, -camera.far, 1.0);
    let before_near = projection * nalgebra::Vector4::new(0.0, 0.0, -camera.near * 0.5, 1.0);

    assert!(near.z.abs() < 1e-5);
    assert!((far.z / far.w - 1.0).abs() < 1e-5);
    assert!(before_near.z < 0.0);
}

#[test]
fn sky_view_ignores_world_position_but_preserves_camera_effects() {
    let mut camera = Camera::new(Point3::new(12.0, 80.0, -4.0), 16.0 / 9.0);
    let stable = camera.sky_view_matrix();

    camera.position = Point3::new(-35.0, 7.0, 128.0);
    let moved = camera.sky_view_matrix();
    assert!((stable - moved).abs().max() < 1e-5);

    camera.view_bobbing = true;
    camera.bob_amount = 0.8;
    camera.bob_phase = 0.35;
    camera.hurt_time = 0.2;
    camera.prev_hurt_time = 0.2;

    assert!((camera.sky_view_matrix() - moved).abs().max() > 1e-3);
}

#[test]
fn protocol_yaw_remains_continuous_across_a_full_turn() {
    let mut camera = Camera::new(Point3::origin(), 1.0);
    camera.set_from_mc_yaw(179.0);
    camera.add_mc_yaw(2.0);

    assert_eq!(camera.mc_yaw_degrees(), 181.0);
}
