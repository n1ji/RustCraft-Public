use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

pub(super) fn entity_billboard(
    entity: &crate::entity::Entity,
    session: &crate::client::session::SessionState,
    world: &crate::world::World,
    slim: bool,
    cape_ready: bool,
    fishing_line_start: Option<[f32; 3]>,
) -> crate::render::scene::EntityBillboard {
    use crate::entity::{EntityData, EntityType};
    let (width, height) = match entity.hanging.as_ref() {
        Some(crate::entity::HangingEntityData::Painting { title, .. }) => {
            let (width, height, _, _) = crate::entity::painting_art(title);
            (width as f32 / 16.0, height as f32 / 16.0)
        }
        Some(crate::entity::HangingEntityData::ItemFrame { .. }) => (0.75, 0.75),
        Some(crate::entity::HangingEntityData::LeashKnot { .. }) => (0.375, 0.5),
        None => entity.entity_type.bounding_box(),
    };
    let kind = match entity.entity_type {
        EntityType::Player => crate::render::scene::EntityBillboardKind::Player,
        EntityType::ArmorStand => crate::render::scene::EntityBillboardKind::Other,
        EntityType::Item => crate::render::scene::EntityBillboardKind::Item,
        EntityType::XPOrb => crate::render::scene::EntityBillboardKind::XpOrb,
        EntityType::Arrow
        | EntityType::Snowball
        | EntityType::ThrownEgg
        | EntityType::Fireball
        | EntityType::SmallFireball
        | EntityType::EnderPearl
        | EntityType::ThrownPotion
        | EntityType::ThrownExpBottle
        | EntityType::EnderEye
        | EntityType::WitherSkull
        | EntityType::FireworkRocket => crate::render::scene::EntityBillboardKind::Projectile,
        ty if ty.is_mob() && !ty.is_passive() => crate::render::scene::EntityBillboardKind::Hostile,
        ty if ty.is_passive() => crate::render::scene::EntityBillboardKind::Passive,
        _ => crate::render::scene::EntityBillboardKind::Other,
    };

    let custom_name = entity.metadata.iter().find_map(|entry| {
        (entry.index == 2)
            .then_some(&entry.value)
            .and_then(|value| match value {
                crate::net::metadata::MetadataValue::String(name) if !name.is_empty() => {
                    Some(name.clone())
                }
                _ => None,
            })
    });
    let (name, health) = match &entity.data {
        EntityData::Player { name, .. } => {
            let display = entity
                .uuid
                .as_deref()
                .and_then(|uuid| session.player_list.get(uuid))
                .map(|player| session.player_display_name(player))
                .unwrap_or_else(|| session.decorate_player_name(name));
            (Some(display), None)
        }
        EntityData::Mob { health, max_health }
        | EntityData::Living {
            health, max_health, ..
        } => (custom_name.clone(), Some((*health, *max_health))),
        EntityData::Item {
            item_id,
            count,
            damage,
            ..
        } => (Some(format!("{}x{}:{}", count, item_id, damage)), None),
        EntityData::XPOrb { value } => (Some(format!("{} XP", value)), None),
        EntityData::None => (custom_name.clone(), None),
    };
    let skin_key =
        matches!(entity.entity_type, EntityType::Player).then(|| player_skin_key(entity));

    let entity_flags = entity
        .metadata
        .iter()
        .find_map(|entry| {
            (entry.index == 0)
                .then_some(&entry.value)
                .and_then(|value| {
                    if let crate::net::metadata::MetadataValue::Byte(flags) = value {
                        Some(*flags as u8)
                    } else {
                        None
                    }
                })
        })
        .unwrap_or(0);
    let base_invisible = entity_flags & 0x20 != 0;
    let sneaking = entity_flags & 0x02 != 0;
    let always_render_name = entity.metadata.iter().any(|entry| {
        entry.index == 3
            && matches!(entry.value, crate::net::metadata::MetadataValue::Byte(value) if value != 0)
    });
    let name_visible = match &entity.data {
        EntityData::Player { name: _name, .. } => {
            !base_invisible
                && !entity.active_effects.iter().any(|e| e.effect_id == 14)
                && entity.vehicle_id.is_none()
        }
        // The old path accidentally made nametags a player-only feature:
        // every non-player entity was additionally gated by metadata index 3.
        // Keep item/XP labels out of the world, but let named mobs and other
        // visible entities use the same backend-neutral GUI path. Servers
        // differ in whether they send the optional "always render" byte.
        EntityData::Item { .. } | EntityData::XPOrb { .. } => false,
        _ => {
            name.is_some()
                && !base_invisible
                && !entity.active_effects.iter().any(|e| e.effect_id == 14)
                && entity.vehicle_id.is_none()
                && (always_render_name || entity.entity_type.is_mob())
        }
    };

    let eye_height = entity.entity_type.eye_height()
        - if entity.entity_type == EntityType::Player && sneaking {
            0.08
        } else {
            0.0
        };
    let light = world.light_at_world(
        entity.render_position.x.floor() as i32,
        (entity.render_position.y + eye_height).floor() as i32,
        entity.render_position.z.floor() as i32,
    );
    crate::render::scene::EntityBillboard {
        entity_id: entity.entity_id,
        position: [
            entity.render_position.x,
            entity.render_position.y,
            entity.render_position.z,
        ],
        sky_light: light.sky,
        block_light: light.block,
        height,
        width,
        name,
        kind,
        entity_type: entity.entity_type,
        health,
        held_item: entity.current_item.map(|id| id.max(0) as u16),
        equipment: {
            let mut eq: [Option<(u16, u16)>; 5] = Default::default();
            for (i, slot) in entity.equipment.iter().enumerate().take(5) {
                eq[i] = slot
                    .as_ref()
                    .map(|s| (s.item_id.max(0) as u16, s.damage.max(0) as u16));
            }
            eq
        },
        item_id: if let crate::entity::EntityData::Item { item_id, .. } = &entity.data {
            Some(*item_id)
        } else {
            None
        },
        item_damage: if let crate::entity::EntityData::Item { damage, .. } = &entity.data {
            Some(*damage)
        } else {
            None
        },
        item_nbt: if let crate::entity::EntityData::Item { nbt, .. } = &entity.data {
            nbt.clone()
        } else {
            None
        },
        swing_progress: (entity.swing_time / 0.35).clamp(0.0, 1.0),
        skin_key,
        slim,
        skin_parts_mask: entity.skin_parts,
        has_cape: cape_ready,
        cape_rotation: cape_rotation(
            entity.render_chasing_position,
            entity.render_position,
            entity.body_yaw,
            entity.prev_distance_walked_modified,
            entity.distance_walked_modified,
            entity.prev_camera_yaw,
            entity.camera_yaw,
            1.0,
            sneaking,
        ),
        yaw: entity.body_yaw,
        pitch: entity.pitch,
        head_yaw: entity.head_yaw,
        limb_swing: entity.limb_swing,
        limb_swing_amount: entity.limb_swing_amount,
        sneaking,
        blocking: entity.using_item
            && matches!(entity.current_item, Some(267 | 268 | 272 | 276 | 283)),
        invisible: base_invisible
            || entity
                .active_effects
                .iter()
                .any(|effect| effect.effect_id == 14),
        riding: entity.vehicle_id.is_some(),
        name_visible,
        age_ticks: entity.ticks_alive as f32,
        hover_start: entity.hover_start,
        velocity: [entity.velocity.x, entity.velocity.y, entity.velocity.z],
        hurt_alpha: (entity.hurt_time / 0.45).clamp(0.0, 1.0),
        // `death_time` counts down, while the model needs elapsed time. Status
        // 3 is terminal, so retain the completed corpse pose until the server
        // finally destroys the tracked entity.
        death_alpha: (entity.last_status == Some(3))
            .then(|| 1.0 - entity.death_time.clamp(0.0, 1.0))
            .unwrap_or(0.0),
        swing_alpha: (entity.swing_time / 0.35).clamp(0.0, 1.0),
        critical_alpha: (entity.critical_time / 0.45).clamp(0.0, 1.0),
        visual: entity.visual,
        hanging: entity.hanging.clone(),
        fishing_line_start,
        painting_lights: painting_panel_lights(entity, world),
    }
}

pub(super) fn painting_panel_lights(
    entity: &crate::entity::Entity,
    world: &crate::world::World,
) -> Vec<[u8; 2]> {
    let Some(crate::entity::HangingEntityData::Painting { title, facing, .. }) =
        entity.hanging.as_ref()
    else {
        return Vec::new();
    };
    let (width, height, _, _) = crate::entity::painting_art(title);
    let mut lights = Vec::with_capacity((width * height / 256) as usize);
    let left = -(width as f32) * 0.5;
    let bottom = -(height as f32) * 0.5;
    for i in 0..width / 16 {
        for j in 0..height / 16 {
            let panel_x = (left + (i * 16 + 8) as f32) / 16.0;
            let panel_y = (bottom + (j * 16 + 8) as f32) / 16.0;
            let mut x = entity.position.x.floor() as i32;
            let y = (entity.position.y + panel_y).floor() as i32;
            let mut z = entity.position.z.floor() as i32;
            match facing % 4 {
                2 => x = (entity.position.x + panel_x).floor() as i32, // north
                1 => z = (entity.position.z - panel_x).floor() as i32, // west
                0 => x = (entity.position.x - panel_x).floor() as i32, // south
                _ => z = (entity.position.z + panel_x).floor() as i32, // east
            }
            let light = world.light_at_world(x, y, z);
            lights.push([light.sky, light.block]);
        }
    }
    lights
}

pub(super) fn fishing_line_start(
    hook: &crate::entity::Entity,
    entities: &crate::entity::EntityManager,
    player: &crate::client::player::Player,
    local_entity_id: Option<i32>,
    local_swing_progress: f32,
) -> Option<[f32; 3]> {
    let owner_id = hook.fishing_owner?;
    if local_entity_id == Some(owner_id) && player.camera_mode == 0 {
        let rotate_pitch = |v: nalgebra::Vector3<f32>, angle: f32| {
            let (sin, cos) = angle.sin_cos();
            nalgebra::Vector3::new(v.x, v.y * cos + v.z * sin, v.z * cos - v.y * sin)
        };
        let rotate_yaw = |v: nalgebra::Vector3<f32>, angle: f32| {
            let (sin, cos) = angle.sin_cos();
            nalgebra::Vector3::new(v.x * cos + v.z * sin, v.y, v.z * cos - v.x * sin)
        };
        let swing = (local_swing_progress.clamp(0.0, 1.0).sqrt() * std::f32::consts::PI).sin();
        let mut offset = nalgebra::Vector3::new(-0.36_f32, 0.03, 0.35);
        offset = rotate_pitch(offset, -player.camera.mc_pitch_degrees().to_radians());
        offset = rotate_yaw(offset, -player.camera.mc_yaw_degrees().to_radians());
        offset = rotate_yaw(offset, swing * 0.5);
        offset = rotate_pitch(offset, -swing * 0.7);
        return Some([
            player.render_position.x + offset.x,
            player.render_position.y + offset.y,
            player.render_position.z + offset.z,
        ]);
    }

    let (position, body_yaw, sneaking) = if local_entity_id == Some(owner_id) {
        (player.render_position, player.body_yaw, player.sneaking)
    } else {
        let owner = entities.get(owner_id)?;
        let sneaking = owner.metadata.iter().any(|entry| {
            entry.index == 0
                && matches!(
                    entry.value,
                    crate::net::metadata::MetadataValue::Byte(flags) if flags as u8 & 0x02 != 0
                )
        });
        (owner.render_position, owner.body_yaw, sneaking)
    };
    let yaw = body_yaw.to_radians();
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let eye_height = 1.62 - if sneaking { 0.08 } else { 0.0 };
    Some([
        position.x - cos_yaw * 0.35 - sin_yaw * 0.8,
        position.y + eye_height - 0.45 - if sneaking { 0.1875 } else { 0.0 },
        position.z - sin_yaw * 0.35 + cos_yaw * 0.8,
    ])
}
pub(super) fn player_skin_key(entity: &crate::entity::Entity) -> String {
    let name = match &entity.data {
        crate::entity::EntityData::Player { name, .. } => Some(name.as_str()),
        _ => None,
    };
    format!(
        "player/{}",
        entity.uuid.as_deref().or(name).unwrap_or("default")
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cape_rotation(
    chasing: nalgebra::Point3<f32>,
    position: nalgebra::Point3<f32>,
    body_yaw: f32,
    prev_distance: f32,
    distance: f32,
    prev_camera_yaw: f32,
    camera_yaw: f32,
    partial_tick: f32,
    sneaking: bool,
) -> [f32; 3] {
    let delta = chasing - position;
    let yaw = body_yaw.to_radians();
    let sin_yaw = yaw.sin();
    let neg_cos_yaw = -yaw.cos();
    let vertical = (delta.y * 10.0).clamp(-6.0, 32.0);
    let forward = ((delta.x * sin_yaw + delta.z * neg_cos_yaw) * 100.0).max(0.0);
    let sideways = (delta.x * neg_cos_yaw - delta.z * sin_yaw) * 100.0;
    let walk_distance = prev_distance + (distance - prev_distance) * partial_tick;
    let camera_yaw = prev_camera_yaw + (camera_yaw - prev_camera_yaw) * partial_tick;
    let walk = (walk_distance * 6.0).sin() * 32.0 * camera_yaw;
    let x = 6.0 + forward * 0.5 + vertical + walk + if sneaking { 25.0 } else { 0.0 };

    [
        -x.to_radians(),
        std::f32::consts::PI - (sideways * 0.5).to_radians(),
        -(sideways * 0.5).to_radians(),
    ]
}

pub(super) fn skull_skin_key(skull: &crate::world::SkullBlockEntity) -> String {
    if skull.skull_type == 3 {
        format!(
            "player/skull/{}/{}",
            skull
                .owner_uuid
                .as_deref()
                .or(skull.owner_name.as_deref())
                .unwrap_or("default"),
            skull
                .skin_property
                .as_deref()
                .map(stable_short_hash)
                .unwrap_or(0)
        )
    } else {
        match skull.skull_type {
            1 => "wither_skeleton",
            2 => "zombie",
            4 => "creeper",
            _ => "skeleton",
        }
        .to_string()
    }
}

pub(super) fn stable_short_hash(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn interpolated_chest_lid_angle(
    world: &crate::world::World,
    position: (i32, i32, i32),
    block: crate::world::block::Block,
    alpha: f32,
) -> f32 {
    let (x, y, z) = position;
    let Some(chest) = world.chests.get(&position) else {
        return 0.0;
    };
    let mut lid_angle = chest.prev_lid_angle + (chest.lid_angle - chest.prev_lid_angle) * alpha;
    let adjacent_position = if block != crate::world::block::Block::EnderChest {
        if world.get_block(x + 1, y, z) == block {
            Some((x + 1, y, z))
        } else if world.get_block(x, y, z + 1) == block {
            Some((x, y, z + 1))
        } else {
            None
        }
    } else {
        None
    };
    if let Some(adjacent) = adjacent_position.and_then(|position| world.chests.get(&position)) {
        lid_angle = lid_angle
            .max(adjacent.prev_lid_angle + (adjacent.lid_angle - adjacent.prev_lid_angle) * alpha);
    }
    lid_angle
}

pub(super) fn mod_config_row(
    entry: crate::scripting::ConfigEntrySnapshot,
) -> crate::render::scene::ModConfigRow {
    use crate::scripting::{ConfigEntryKind, ConfigValue};

    let is_default = entry.value == entry.default_value;
    let (value, can_previous, can_next) = match (&entry.kind, &entry.value) {
        (ConfigEntryKind::Boolean, ConfigValue::Boolean(value)) => {
            (if *value { "On" } else { "Off" }.to_string(), true, true)
        }
        (ConfigEntryKind::Number { min, max, .. }, ConfigValue::Number(value)) => (
            format_config_number(*value),
            *value > *min + f64::EPSILON,
            *value < *max - f64::EPSILON,
        ),
        (ConfigEntryKind::Choice { options }, ConfigValue::Choice(value)) => {
            let label = options
                .iter()
                .find(|option| option.value == *value)
                .map(|option| option.label.clone())
                .unwrap_or_else(|| value.clone());
            (label, options.len() > 1, options.len() > 1)
        }
        _ => ("Invalid value".to_string(), false, false),
    };
    crate::render::scene::ModConfigRow {
        key: entry.key,
        label: entry.label,
        description: entry.description,
        value,
        is_default,
        can_previous,
        can_next,
    }
}

pub(super) fn format_config_number(value: f64) -> String {
    let formatted = format!("{value:.4}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}
