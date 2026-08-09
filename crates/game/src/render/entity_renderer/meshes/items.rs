use crate::client::player::Camera;
use crate::render::entity::atlas::MobTextureRegion;
use crate::render::entity::mesh::EntityVertex;
use crate::render::scene::EntityBillboard;

use super::common::*;

pub(crate) fn packed_light_for(sky_light: u8, block_light: u8) -> f32 {
    16.0 + sky_light.min(15) as f32 * 16.0 + block_light.min(15) as f32
}

/// Return the transparent queue key used by world chunks: farther geometry
/// sorts first, while overlays can reserve `u64::MAX` for the final layer.
/// Batched item/particle meshes use their farthest vertex so one batch cannot
/// pull nearer alpha geometry in front of an entire distant batch.
pub(crate) fn transparent_sort_key(
    camera: &Camera,
    vertices: &[crate::render::entity::mesh::EntityVertex],
) -> u64 {
    let distance = vertices
        .iter()
        .map(|vertex| {
            let dx = vertex.position[0] - camera.position.x;
            let dy = vertex.position[1] - camera.position.y;
            let dz = vertex.position[2] - camera.position.z;
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .fold(0.0, f32::max)
        .min(1_000_000.0);
    u64::MAX - (distance * 1024.0) as u64
}

pub(crate) fn append_entity_quad(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    region: &crate::render::entity::atlas::MobTextureRegion,
    local_uv: [f32; 4],
    color: [f32; 4],
) {
    let base = vertices.len() as u32;
    let [u0, v0, u1, v1] = local_uv;
    for (position, [u, v]) in corners
        .into_iter()
        .zip([[u0, v1], [u1, v1], [u1, v0], [u0, v0]])
    {
        let (u, v) = region.local_to_atlas(u, v);
        vertices.push(crate::render::entity::mesh::EntityVertex {
            position,
            normal,
            uv: [u, v],
            color,
        });
    }
    indices.extend_from_slice(&[
        base,
        base + 1,
        base + 2,
        base,
        base + 2,
        base + 3,
        base,
        base + 2,
        base + 1,
        base,
        base + 3,
        base + 2,
    ]);
}

pub(crate) fn append_entity_box(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    transform: nalgebra::Matrix4<f32>,
    center: [f32; 3],
    size: [f32; 3],
    region: &crate::render::entity::atlas::MobTextureRegion,
    color: [f32; 4],
) {
    use nalgebra::{Point3, Vector3};

    let half = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let points = [
        [
            center[0] - half[0],
            center[1] - half[1],
            center[2] - half[2],
        ],
        [
            center[0] + half[0],
            center[1] - half[1],
            center[2] - half[2],
        ],
        [
            center[0] + half[0],
            center[1] + half[1],
            center[2] - half[2],
        ],
        [
            center[0] - half[0],
            center[1] + half[1],
            center[2] - half[2],
        ],
        [
            center[0] - half[0],
            center[1] - half[1],
            center[2] + half[2],
        ],
        [
            center[0] + half[0],
            center[1] - half[1],
            center[2] + half[2],
        ],
        [
            center[0] + half[0],
            center[1] + half[1],
            center[2] + half[2],
        ],
        [
            center[0] - half[0],
            center[1] + half[1],
            center[2] + half[2],
        ],
    ];
    let faces = [
        ([1, 0, 3, 2], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([3, 7, 6, 2], [0.0, 1.0, 0.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([0, 4, 7, 3], [-1.0, 0.0, 0.0]),
        ([5, 1, 2, 6], [1.0, 0.0, 0.0]),
    ];
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    for (face, normal) in faces {
        let corners = face.map(|index| {
            let point = transform.transform_point(&Point3::from(points[index]));
            [point.x, point.y, point.z]
        });
        let normal = (normal_matrix * Vector3::from(normal)).normalize();
        append_entity_quad(
            vertices,
            indices,
            corners,
            [normal.x, normal.y, normal.z],
            region,
            [0.0, 0.0, 1.0, 1.0],
            color,
        );
    }
}

pub(crate) fn append_item_frame_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    billboard: &EntityBillboard,
    wood: &crate::render::entity::atlas::MobTextureRegion,
    background: &crate::render::entity::atlas::MobTextureRegion,
) {
    use nalgebra::{Matrix4, Rotation3, Vector3};

    let Some(crate::entity::HangingEntityData::ItemFrame {
        anchor,
        facing,
        displayed_item,
        ..
    }) = billboard.hanging.as_ref()
    else {
        return;
    };
    let transform = Matrix4::new_translation(&Vector3::new(
        anchor[0] as f32 + 0.5,
        anchor[1] as f32 + 0.5,
        anchor[2] as f32 + 0.5,
    )) * Rotation3::from_axis_angle(
        &Vector3::y_axis(),
        (180.0 - *facing as f32 * 90.0).to_radians(),
    )
    .to_homogeneous();
    let color = [
        1.0,
        1.0,
        1.0,
        packed_light_for(billboard.sky_light, billboard.block_light),
    ];
    let has_map = displayed_item
        .as_ref()
        .is_some_and(|item| item.item_id_u16() == 358);
    let (outer_size, bar_center, inner_size, back_depth, back_thickness) = if has_map {
        (1.0, 0.46875, 0.875, 0.46878125, 0.0624375)
    } else {
        (0.75, 0.34375, 0.625, 0.484375, 0.03125)
    };
    for y in [-bar_center, bar_center] {
        append_entity_box(
            vertices,
            indices,
            transform,
            [0.0, y, 0.46875],
            [outer_size, 0.0625, 0.0625],
            wood,
            color,
        );
    }
    for x in [-bar_center, bar_center] {
        append_entity_box(
            vertices,
            indices,
            transform,
            [x, 0.0, 0.46875],
            [0.0625, inner_size, 0.0625],
            wood,
            color,
        );
    }
    append_entity_box(
        vertices,
        indices,
        transform,
        [0.0, 0.0, back_depth],
        [inner_size, inner_size, back_thickness],
        background,
        color,
    );
}

pub(crate) fn append_painting_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    billboard: &EntityBillboard,
    region: &crate::render::entity::atlas::MobTextureRegion,
) {
    use nalgebra::{Matrix4, Rotation3, Vector3};

    let Some(crate::entity::HangingEntityData::Painting { title, facing, .. }) =
        billboard.hanging.as_ref()
    else {
        return;
    };
    let (width, height, texture_u, texture_v) = crate::entity::painting_art(title);
    let transform = Matrix4::new_translation(&Vector3::from(billboard.position))
        * Rotation3::from_axis_angle(
            &Vector3::y_axis(),
            (180.0 - *facing as f32 * 90.0).to_radians(),
        )
        .to_homogeneous();
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    let left = -(width as f32) * 0.5 / 16.0;
    let bottom = -(height as f32) * 0.5 / 16.0;
    let mut panel = 0;
    for x in 0..width / 16 {
        for y in 0..height / 16 {
            let [sky, block] = billboard
                .painting_lights
                .get(panel)
                .copied()
                .unwrap_or([billboard.sky_light, billboard.block_light]);
            panel += 1;
            let color = [1.0, 1.0, 1.0, packed_light_for(sky, block)];
            let x0 = left + x as f32;
            let x1 = x0 + 1.0;
            let y0 = bottom + y as f32;
            let y1 = y0 + 1.0;
            let z0 = -1.0 / 32.0;
            let z1 = 1.0 / 32.0;
            let mut face = |points: [[f32; 3]; 4], normal: [f32; 3], uv: [f32; 4]| {
                let corners = points.map(|point| {
                    let point = transform.transform_point(&nalgebra::Point3::from(point));
                    [point.x, point.y, point.z]
                });
                let normal = (normal_matrix * nalgebra::Vector3::from(normal)).normalize();
                append_entity_quad(
                    vertices,
                    indices,
                    corners,
                    [normal.x, normal.y, normal.z],
                    region,
                    uv,
                    color,
                );
            };
            face(
                [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
                [0.0, 0.0, -1.0],
                [
                    (texture_u + width - (x + 1) * 16) as f32 / 256.0,
                    (texture_v + height - (y + 1) * 16) as f32 / 256.0,
                    (texture_u + width - x * 16) as f32 / 256.0,
                    (texture_v + height - y * 16) as f32 / 256.0,
                ],
            );
            let backing = [0.75, 0.0, 0.8125, 0.0625];
            for (points, normal) in [
                (
                    [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
                    [0.0, 0.0, 1.0],
                ),
                (
                    [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
                    [0.0, 1.0, 0.0],
                ),
                (
                    [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
                    [0.0, -1.0, 0.0],
                ),
                (
                    [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
                    [-1.0, 0.0, 0.0],
                ),
                (
                    [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
                    [1.0, 0.0, 0.0],
                ),
            ] {
                face(points, normal, backing);
            }
        }
    }
}

pub(crate) fn append_item_frame_display(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    billboard: &EntityBillboard,
) {
    use nalgebra::{Matrix4, Rotation3, Scale3, Vector3};

    let Some(crate::entity::HangingEntityData::ItemFrame {
        anchor,
        facing,
        displayed_item: Some(item),
        rotation,
    }) = billboard.hanging.as_ref()
    else {
        return;
    };
    if item.is_empty() {
        return;
    }
    let transform = Matrix4::new_translation(&Vector3::new(
        anchor[0] as f32 + 0.5,
        anchor[1] as f32 + 0.5,
        anchor[2] as f32 + 0.5,
    )) * Rotation3::from_axis_angle(
        &Vector3::y_axis(),
        (180.0 - (*facing % 4) as f32 * 90.0).to_radians(),
    )
    .to_homogeneous()
        * Matrix4::new_translation(&Vector3::new(0.0, 0.0, 0.44))
        * Rotation3::from_axis_angle(
            &Vector3::z_axis(),
            (*rotation % 8) as f32 * std::f32::consts::FRAC_PI_4,
        )
        .to_homogeneous()
        * Scale3::new(0.5, 0.5, 0.5).to_homogeneous();
    append_transformed_item_sprite(
        vertices,
        indices,
        item.item_id_u16(),
        item.damage.max(0) as u16,
        transform,
        packed_light_for(billboard.sky_light, billboard.block_light),
    );
}

pub(crate) fn append_dropped_item_sprite(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    billboard: &EntityBillboard,
) {
    use nalgebra::{Matrix4, Rotation3, Scale3, Vector3};

    let Some(item_id) = billboard.item_id else {
        return;
    };
    let bob = (billboard.age_ticks / 10.0 + billboard.hover_start).sin() * 0.1 + 0.1;
    let rotation = (billboard.age_ticks / 20.0 + billboard.hover_start) * std::f32::consts::PI;
    let transform = Matrix4::new_translation(&Vector3::new(
        billboard.position[0],
        billboard.position[1] + bob + 0.06,
        billboard.position[2],
    )) * Rotation3::from_axis_angle(&Vector3::y_axis(), rotation).to_homogeneous()
        * Scale3::new(0.35, 0.35, 0.35).to_homogeneous();
    append_transformed_item_sprite(
        vertices,
        indices,
        item_id,
        billboard.item_damage.unwrap_or(0),
        transform,
        packed_light_for(billboard.sky_light, billboard.block_light),
    );
}

pub(crate) fn append_transformed_item_sprite(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    item_id: u16,
    damage: u16,
    transform: nalgebra::Matrix4<f32>,
    packed_light: f32,
) {
    if let Some(mesh) = crate::render::item_icons::get_cached_extruded_mesh(item_id, damage) {
        if append_transformed_extruded_item(
            vertices,
            indices,
            item_id,
            damage,
            transform,
            packed_light,
            &mesh,
        ) {
            return;
        }
    }

    // Loading may briefly precede the extrusion cache. Keep that path
    // three-dimensional as well instead of regressing to a zero-thickness
    // billboard.
    append_transformed_item_slab(vertices, indices, item_id, damage, transform, packed_light);
}

fn append_transformed_extruded_item(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    item_id: u16,
    damage: u16,
    transform: nalgebra::Matrix4<f32>,
    packed_light: f32,
    mesh: &crate::render::item_icons::ExtrudedMesh,
) -> bool {
    use nalgebra::{Point3, Vector3};

    if mesh.vertices.is_empty() || mesh.indices.is_empty() {
        return false;
    }
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    let before = indices.len();
    for layer in crate::render::item_icons::item_icon_layers(item_id, damage) {
        let Some(atlas_index) = crate::render::item_icons::item_icon_entry_index(layer.path) else {
            continue;
        };
        let [u0, v0, u1, v1] = crate::render::item_icons::item_icon_uv_rect(atlas_index);
        let base = vertices.len() as u32;
        for source in &mesh.vertices {
            let point = transform.transform_point(&Point3::from(source.pos));
            let normal = (normal_matrix * Vector3::from(source.normal)).normalize();
            vertices.push(crate::render::entity::mesh::EntityVertex {
                position: [point.x, point.y, point.z],
                normal: [normal.x, normal.y, normal.z],
                uv: [u0 + source.uv[0] * (u1 - u0), v0 + source.uv[1] * (v1 - v0)],
                color: [layer.color[0], layer.color[1], layer.color[2], packed_light],
            });
        }
        indices.extend(mesh.indices.iter().map(|index| base + index));
    }
    indices.len() > before
}

fn append_transformed_item_slab(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    item_id: u16,
    damage: u16,
    transform: nalgebra::Matrix4<f32>,
    packed_light: f32,
) {
    use nalgebra::{Point3, Vector3};

    const HALF_THICKNESS: f32 = 0.03125;
    let faces = [
        (
            [0.0, 0.0, 1.0],
            [
                [0.5, 0.5, HALF_THICKNESS],
                [-0.5, 0.5, HALF_THICKNESS],
                [-0.5, -0.5, HALF_THICKNESS],
                [0.5, -0.5, HALF_THICKNESS],
            ],
            [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [-0.5, 0.5, -HALF_THICKNESS],
                [0.5, 0.5, -HALF_THICKNESS],
                [0.5, -0.5, -HALF_THICKNESS],
                [-0.5, -0.5, -HALF_THICKNESS],
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, 0.5, -HALF_THICKNESS],
                [-0.5, 0.5, HALF_THICKNESS],
                [-0.5, -0.5, HALF_THICKNESS],
                [-0.5, -0.5, -HALF_THICKNESS],
            ],
            [[0.0, 0.0]; 4],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, 0.5, HALF_THICKNESS],
                [0.5, 0.5, -HALF_THICKNESS],
                [0.5, -0.5, -HALF_THICKNESS],
                [0.5, -0.5, HALF_THICKNESS],
            ],
            [[1.0, 0.0]; 4],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [0.5, 0.5, HALF_THICKNESS],
                [-0.5, 0.5, HALF_THICKNESS],
                [-0.5, 0.5, -HALF_THICKNESS],
                [0.5, 0.5, -HALF_THICKNESS],
            ],
            [[0.5, 0.0]; 4],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [0.5, -0.5, -HALF_THICKNESS],
                [-0.5, -0.5, -HALF_THICKNESS],
                [-0.5, -0.5, HALF_THICKNESS],
                [0.5, -0.5, HALF_THICKNESS],
            ],
            [[0.5, 1.0]; 4],
        ),
    ];
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    for layer in crate::render::item_icons::item_icon_layers(item_id, damage) {
        let Some(atlas_index) = crate::render::item_icons::item_icon_entry_index(layer.path) else {
            continue;
        };
        let [u0, v0, u1, v1] = crate::render::item_icons::item_icon_uv_rect(atlas_index);
        for (source_normal, source_points, local_uvs) in faces {
            let normal = (normal_matrix * Vector3::from(source_normal)).normalize();
            let base = vertices.len() as u32;
            for (source_point, local_uv) in source_points.into_iter().zip(local_uvs) {
                let point = transform.transform_point(&Point3::from(source_point));
                vertices.push(crate::render::entity::mesh::EntityVertex {
                    position: [point.x, point.y, point.z],
                    normal: [normal.x, normal.y, normal.z],
                    uv: [u0 + local_uv[0] * (u1 - u0), v0 + local_uv[1] * (v1 - v0)],
                    color: [layer.color[0], layer.color[1], layer.color[2], packed_light],
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

pub(crate) fn append_camera_facing_item_sprite(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    position: [f32; 3],
    item_id: u16,
    damage: u16,
    scale: f32,
    camera: &Camera,
    packed_light: f32,
) {
    let right = camera.right * (0.25 * scale);
    let up = camera.up * (0.25 * scale);
    let center = nalgebra::Vector3::from(position);
    let points = [
        center - right - up,
        center + right - up,
        center + right + up,
        center - right + up,
    ]
    .map(|point| [point.x, point.y, point.z]);
    let normal = if camera.reverse_view {
        camera.front
    } else {
        -camera.front
    };
    append_item_sprite_layers(
        vertices,
        indices,
        item_id,
        damage,
        points,
        [normal.x, normal.y, normal.z],
        packed_light,
    );
}

pub(crate) fn append_item_sprite_layers(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    item_id: u16,
    damage: u16,
    points: [[f32; 3]; 4],
    normal: [f32; 3],
    packed_light: f32,
) {
    for layer in crate::render::item_icons::item_icon_layers(item_id, damage) {
        let Some(index) = crate::render::item_icons::item_icon_entry_index(layer.path) else {
            continue;
        };
        let [u0, v0, u1, v1] = crate::render::item_icons::item_icon_uv_rect(index);
        let base = vertices.len() as u32;
        for (position, uv) in points
            .into_iter()
            .zip([[u0, v1], [u1, v1], [u1, v0], [u0, v0]])
        {
            vertices.push(crate::render::entity::mesh::EntityVertex {
                position,
                normal,
                uv,
                color: [layer.color[0], layer.color[1], layer.color[2], packed_light],
            });
        }
        indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base,
            base + 2,
            base + 3,
            base,
            base + 2,
            base + 1,
            base,
            base + 3,
            base + 2,
        ]);
    }
}

pub(crate) fn projectile_item(entity_type: crate::entity::EntityType) -> Option<(u16, u16, f32)> {
    use crate::entity::EntityType;
    Some(match entity_type {
        EntityType::Snowball => (332, 0, 1.0),
        EntityType::ThrownEgg => (344, 0, 1.0),
        EntityType::EnderPearl => (368, 0, 1.0),
        EntityType::ThrownPotion => (373, 0, 1.0),
        EntityType::ThrownExpBottle => (384, 0, 1.0),
        EntityType::EnderEye => (381, 0, 1.0),
        EntityType::Fireball => (385, 0, 4.0),
        EntityType::SmallFireball => (385, 0, 1.0),
        _ => return None,
    })
}

pub(crate) fn append_fishing_hook_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    billboard: &EntityBillboard,
    camera: &Camera,
    hook_region: &crate::render::entity::atlas::MobTextureRegion,
    white_region: &crate::render::entity::atlas::MobTextureRegion,
) {
    use nalgebra::Vector3;

    let center = Vector3::from(billboard.position);
    let mut forward = camera.position - nalgebra::Point3::from(center);
    if forward.norm_squared() < 1.0e-6 {
        forward = Vector3::z();
    } else {
        forward.normalize_mut();
    }
    let mut right = Vector3::y().cross(&forward);
    if right.norm_squared() < 1.0e-6 {
        right = Vector3::x();
    } else {
        right.normalize_mut();
    }
    let up = forward.cross(&right).normalize();
    let half = 0.25;
    let corners = [
        center - right * half - up * half,
        center + right * half - up * half,
        center + right * half + up * half,
        center - right * half + up * half,
    ]
    .map(|point| [point.x, point.y, point.z]);
    let light = packed_light_for(billboard.sky_light, billboard.block_light);
    append_entity_quad(
        vertices,
        indices,
        corners,
        [-forward.x, -forward.y, -forward.z],
        hook_region,
        [0.0625, 0.125, 0.125, 0.1875],
        [1.0, 1.0, 1.0, light],
    );
    let Some(end) = billboard.fishing_line_start else {
        return;
    };
    let start = [center.x, center.y + 0.25, center.z];
    let points = fishing_line_points(start, end);
    for segment in points.windows(2) {
        let previous = Vector3::from(segment[0]);
        let next = Vector3::from(segment[1]);
        let direction = next - previous;
        let mut side = forward.cross(&direction);
        if side.norm_squared() < 1.0e-8 {
            side = right;
        } else {
            side.normalize_mut();
        }
        side *= 0.006;
        append_entity_quad(
            vertices,
            indices,
            [
                (previous - side).into(),
                (next - side).into(),
                (next + side).into(),
                (previous + side).into(),
            ],
            [0.0, 1.0, 0.0],
            white_region,
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, light],
        );
    }
}

pub(crate) fn fishing_line_points(start: [f32; 3], end: [f32; 3]) -> [[f32; 3]; 17] {
    let delta = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
    std::array::from_fn(|step| {
        let progress = step as f32 / 16.0;
        [
            start[0] + delta[0] * progress,
            start[1] + delta[1] * (progress * progress + progress) * 0.5,
            start[2] + delta[2] * progress,
        ]
    })
}

#[cfg(test)]
mod item_mesh_tests {
    use super::*;

    #[test]
    fn transformed_item_mesh_has_real_thickness() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        append_transformed_item_sprite(
            &mut vertices,
            &mut indices,
            276,
            0,
            nalgebra::Matrix4::identity(),
            packed_light_for(15, 0),
        );

        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        let min_z = vertices
            .iter()
            .map(|vertex| vertex.position[2])
            .fold(f32::INFINITY, f32::min);
        let max_z = vertices
            .iter()
            .map(|vertex| vertex.position[2])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(max_z - min_z >= 0.06, "item depth was {min_z}..{max_z}");
        assert!(vertices
            .iter()
            .any(|vertex| vertex.normal[0].abs() > 0.9 || vertex.normal[1].abs() > 0.9));
    }
}
