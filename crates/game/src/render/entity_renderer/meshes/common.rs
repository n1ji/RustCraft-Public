use crate::client::interaction::SelectionBox;
use crate::render::entity::atlas::{EntityTextureAtlas, MobTextureRegion};
use crate::render::entity::mesh::{EntityPose, EntityVertex};
use crate::render::scene::EntityBillboard;

pub(crate) fn append_entity_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    appended_vertices: Vec<crate::render::entity::mesh::EntityVertex>,
    appended_indices: Vec<u32>,
) {
    let base = vertices.len() as u32;
    vertices.extend(appended_vertices);
    indices.extend(appended_indices.into_iter().map(|index| index + base));
}

pub(crate) fn remap_entity_uv(
    vertices: &mut [crate::render::entity::mesh::EntityVertex],
    region: &crate::render::entity::atlas::MobTextureRegion,
) {
    for vertex in vertices {
        let (u, v) = region.local_to_atlas(vertex.uv[0], vertex.uv[1]);
        vertex.uv = [u, v];
    }
}

pub(crate) fn entity_model_cache_key(billboard: &EntityBillboard) -> u64 {
    use std::hash::{Hash, Hasher};

    let visual = billboard.visual;
    let mut hasher = fnv::FnvHasher::default();
    billboard.entity_type.hash(&mut hasher);
    for value in [
        visual.is_child,
        visual.zombie_villager,
        visual.wolf_tamed,
        visual.wolf_angry,
        visual.bat_hanging,
        visual.guardian_elder,
        billboard.slim,
        billboard.has_cape,
    ] {
        hasher.write_u8(value as u8);
    }
    for value in [
        visual.skeleton_type,
        visual.ocelot_skin,
        visual.horse_type,
        visual.villager_profession,
        visual.armor_stand_flags,
        billboard.skin_parts_mask,
    ] {
        hasher.write_u8(value);
    }
    hasher.write_u32(visual.horse_variant);
    hasher.finish()
}

pub(crate) fn entity_pose(billboard: &EntityBillboard) -> crate::render::entity::mesh::EntityPose {
    use crate::entity::EntityType;
    use crate::render::entity::mesh::{AnimationFamily, EntityPose};

    EntityPose {
        model_scale: match billboard.entity_type {
            EntityType::Giant => 6.0,
            EntityType::CaveSpider => 0.7,
            EntityType::Guardian if billboard.visual.guardian_elder => 2.35,
            EntityType::ArmorStand if billboard.visual.armor_stand_flags & 0x01 != 0 => 0.5,
            _ => 1.0,
        },
        animation_family: match billboard.entity_type {
            EntityType::Player
            | EntityType::Zombie
            | EntityType::PigZombie
            | EntityType::Giant
            | EntityType::Skeleton
            | EntityType::Enderman
            | EntityType::Witch
            | EntityType::Villager => AnimationFamily::Biped,
            EntityType::ArmorStand => AnimationFamily::ArmorStand,
            EntityType::Pig
            | EntityType::Sheep
            | EntityType::Cow
            | EntityType::Mooshroom
            | EntityType::Wolf
            | EntityType::Ocelot
            | EntityType::Horse => AnimationFamily::Quadruped,
            _ => AnimationFamily::Generic,
        },
        body_yaw: if billboard.entity_type == EntityType::LeashKnot {
            0.0
        } else {
            (180.0 - billboard.yaw).to_radians()
        },
        head_yaw: if billboard.entity_type == EntityType::LeashKnot {
            0.0
        } else {
            (billboard.yaw - billboard.head_yaw).to_radians()
        },
        pitch: if billboard.entity_type == EntityType::LeashKnot {
            0.0
        } else {
            billboard.pitch.to_radians()
        },
        limb_swing: billboard.limb_swing,
        limb_swing_amount: billboard.limb_swing_amount,
        swing_progress: billboard.swing_alpha,
        death_progress: billboard.death_alpha,
        age_ticks: billboard.age_ticks,
        holding_item: billboard.equipment[0]
            .or_else(|| billboard.held_item.map(|id| (id, 0)))
            .is_some_and(|(id, _)| id != 0),
        armor_stand_rotations: billboard.visual.armor_stand_rotations,
        blocking: billboard.blocking,
        sneaking: billboard.sneaking,
        riding: billboard.riding,
        cape_rotation: billboard.cape_rotation,
    }
}

pub(crate) fn append_player_armor(
    atlas: Option<&EntityTextureAtlas>,
    billboard: &EntityBillboard,
    position: nalgebra::Point3<f32>,
    pose: &crate::render::entity::mesh::EntityPose,
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
) {
    if billboard.entity_type != crate::entity::EntityType::Player {
        return;
    }
    let Some(atlas) = atlas else {
        return;
    };
    for (equipment_slot, equipment) in billboard.equipment.iter().enumerate().skip(1) {
        let Some((item_id, _)) = *equipment else {
            continue;
        };
        let Some((material, armor_slot)) = crate::client::armor::armor_material_and_slot(item_id)
        else {
            continue;
        };
        if equipment_slot != 4 - armor_slot {
            continue;
        }
        let texture_layer = if equipment_slot == 2 { 2 } else { 1 };
        let texture_name = crate::client::armor::armor_texture_name(material, texture_layer);
        let Some(region) = atlas.region_for(texture_name) else {
            continue;
        };
        let (mut armor_vertices, armor_indices) = crate::render::entity::mesh::generate_entity_mesh(
            &crate::render::entity::models::player_armor_model(equipment_slot),
            position,
            pose,
        );
        remap_entity_uv(&mut armor_vertices, region);
        if material == crate::client::armor::ArmorMaterial::Leather {
            for vertex in &mut armor_vertices {
                vertex.color[0] *= 0xa0 as f32 / 255.0;
                vertex.color[1] *= 0x65 as f32 / 255.0;
                vertex.color[2] *= 0x40 as f32 / 255.0;
            }
        }
        append_entity_mesh(vertices, indices, armor_vertices, armor_indices);
    }
}

pub(crate) fn apply_entity_vertex_state(
    vertices: &mut [crate::render::entity::mesh::EntityVertex],
    billboard: &EntityBillboard,
) {
    let hurt = billboard.hurt_alpha.clamp(0.0, 1.0);
    let packed_light =
        16.0 + billboard.sky_light.min(15) as f32 * 16.0 + billboard.block_light.min(15) as f32;
    for vertex in vertices {
        if hurt > 0.0 {
            vertex.color[0] = vertex.color[0] * (1.0 - hurt) + hurt;
            vertex.color[1] *= 1.0 - hurt;
            vertex.color[2] *= 1.0 - hurt;
        }
        vertex.color[3] = packed_light;
    }
}

pub(crate) fn append_selection_wireframes(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    boxes: &[crate::client::interaction::SelectionBox],
    white: &crate::render::entity::atlas::MobTextureRegion,
) {
    let (u, v) = white.local_to_atlas(0.5, 0.5);
    let width = 0.01;
    let inset = 0.002;
    for selection in boxes {
        let min = [
            selection.min[0] - inset,
            selection.min[1] - inset,
            selection.min[2] - inset,
        ];
        let max = [
            selection.max[0] + inset,
            selection.max[1] + inset,
            selection.max[2] + inset,
        ];
        let corners = [
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], min[1], max[2]],
            [min[0], min[1], max[2]],
            [min[0], max[1], min[2]],
            [max[0], max[1], min[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ];
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            append_thick_edge(vertices, indices, corners[a], corners[b], width, [u, v]);
        }
    }
}

pub(crate) fn append_thick_edge(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    start: [f32; 3],
    end: [f32; 3],
    width: f32,
    uv: [f32; 2],
) {
    let direction = nalgebra::Vector3::new(end[0] - start[0], end[1] - start[1], end[2] - start[2]);
    let reference = if direction.y.abs() > 0.9 * direction.norm() {
        nalgebra::Vector3::x()
    } else {
        nalgebra::Vector3::y()
    };
    let side = direction
        .cross(&reference)
        .try_normalize(1.0e-6)
        .unwrap_or_else(nalgebra::Vector3::x)
        * width;
    let points = [
        [start[0] - side.x, start[1] - side.y, start[2] - side.z],
        [start[0] + side.x, start[1] + side.y, start[2] + side.z],
        [end[0] + side.x, end[1] + side.y, end[2] + side.z],
        [end[0] - side.x, end[1] - side.y, end[2] - side.z],
    ];
    let base = vertices.len() as u32;
    vertices.extend(
        points.map(|position| crate::render::entity::mesh::EntityVertex {
            position,
            normal: [0.0; 3],
            uv,
            color: [0.0, 0.0, 0.0, 0.4],
        }),
    );
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

pub(crate) fn append_dig_overlay(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    boxes: &[crate::client::interaction::SelectionBox],
    position: Option<[i32; 3]>,
    region: &crate::render::entity::atlas::MobTextureRegion,
) {
    let fallback;
    let boxes = if boxes.is_empty() {
        let Some([x, y, z]) = position else {
            return;
        };
        fallback = [crate::client::interaction::SelectionBox {
            min: [x as f32, y as f32, z as f32],
            max: [x as f32 + 1.0, y as f32 + 1.0, z as f32 + 1.0],
        }];
        &fallback[..]
    } else {
        boxes
    };
    let outset = 0.001;
    for bounds in boxes {
        let [x0, y0, z0] = bounds.min;
        let [x1, y1, z1] = bounds.max;
        for corners in [
            [
                [x0, y0, z1 + outset],
                [x1, y0, z1 + outset],
                [x1, y1, z1 + outset],
                [x0, y1, z1 + outset],
            ],
            [
                [x1, y0, z0 - outset],
                [x0, y0, z0 - outset],
                [x0, y1, z0 - outset],
                [x1, y1, z0 - outset],
            ],
            [
                [x0 - outset, y0, z0],
                [x0 - outset, y0, z1],
                [x0 - outset, y1, z1],
                [x0 - outset, y1, z0],
            ],
            [
                [x1 + outset, y0, z1],
                [x1 + outset, y0, z0],
                [x1 + outset, y1, z0],
                [x1 + outset, y1, z1],
            ],
            [
                [x0, y1 + outset, z1],
                [x1, y1 + outset, z1],
                [x1, y1 + outset, z0],
                [x0, y1 + outset, z0],
            ],
            [
                [x0, y0 - outset, z0],
                [x1, y0 - outset, z0],
                [x1, y0 - outset, z1],
                [x0, y0 - outset, z1],
            ],
        ] {
            let base = vertices.len() as u32;
            for (point, local_uv) in
                corners
                    .into_iter()
                    .zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
            {
                let (u, v) = region.local_to_atlas(local_uv[0], local_uv[1]);
                vertices.push(crate::render::entity::mesh::EntityVertex {
                    position: point,
                    normal: [0.0; 3],
                    uv: [u, v],
                    color: [1.0, 1.0, 1.0, 0.6],
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}
