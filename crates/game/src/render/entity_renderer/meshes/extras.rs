use crate::render::entity::atlas::{MobTextureRegion, ENTITY_ATLAS_SIZE};
use crate::render::entity::mesh::EntityVertex;
use crate::render::scene::{ChestRenderEntry, EntityBillboard, SignEntry, SkullRenderEntry};
use crate::ui::font::FontRenderer;

use super::common::*;

pub(crate) fn sign_texture_key(position: [i32; 3]) -> String {
    format!("sign_{}_{}_{}", position[0], position[1], position[2])
}

pub(crate) fn sign_text_plane(sign: &crate::render::scene::SignEntry) -> ([f32; 3], [f32; 3]) {
    let [x, y, z] = sign.position;
    if !sign.wall_mounted {
        let angle = ((sign.metadata & 0x0f) as f32) * 22.5_f32.to_radians();
        let normal = [-angle.sin(), 0.0, angle.cos()];
        return (
            [
                x as f32 + 0.5 + normal[0] * (1.0 / 24.0 + 0.005),
                y as f32 + 13.333_333 / 16.0,
                z as f32 + 0.5 + normal[2] * (1.0 / 24.0 + 0.005),
            ],
            normal,
        );
    }
    let normal = match sign.metadata & 0x07 {
        2 => [0.0, 0.0, -1.0],
        3 => [0.0, 0.0, 1.0],
        4 => [-1.0, 0.0, 0.0],
        5 => [1.0, 0.0, 0.0],
        _ => [0.0, 0.0, 1.0],
    };
    (
        [
            x as f32 + 0.5 - normal[0] * (19.0 / 48.0) + normal[0] * 0.005,
            y as f32 + 8.333_333 / 16.0,
            z as f32 + 0.5 - normal[2] * (19.0 / 48.0) + normal[2] * 0.005,
        ],
        normal,
    )
}

pub(crate) fn build_sign_texture(
    font: &mut crate::ui::font::FontRenderer,
    lines: &[String],
) -> (Vec<u8>, u32, u32) {
    const SCALE: u32 = 2;
    let width = 90 * SCALE;
    let height = 40 * SCALE;
    let mut pixels = vec![0; (width * height * 4) as usize];
    let font_size = 10.0 * SCALE as f32;
    for (line_index, line) in lines.iter().enumerate().take(4) {
        let mut glyphs = Vec::new();
        let mut chars = line.chars();
        while let Some(character) = chars.next() {
            if character == '\u{00a7}' {
                chars.next();
                continue;
            }
            glyphs.push(font.font.rasterize(character, font_size));
        }
        let line_width = glyphs
            .iter()
            .map(|(metrics, _)| metrics.advance_width)
            .sum::<f32>()
            .ceil() as i32;
        let mut cursor_x = ((width as i32 - line_width) / 2).max(0);
        let baseline = (line_index as u32 * 10 + 9) as i32 * SCALE as i32;
        for (metrics, bitmap) in glyphs {
            let draw_x = cursor_x + metrics.xmin;
            let draw_y = baseline - metrics.ymin - metrics.height as i32;
            for py in 0..metrics.height {
                for px in 0..metrics.width {
                    let alpha = bitmap[py * metrics.width + px];
                    let tx = draw_x + px as i32;
                    let ty = draw_y + py as i32;
                    if alpha == 0 || tx < 0 || ty < 0 || tx >= width as i32 || ty >= height as i32 {
                        continue;
                    }
                    let offset = ((ty as u32 * width + tx as u32) * 4) as usize;
                    pixels[offset + 3] = alpha;
                }
            }
            cursor_x += (metrics.advance_width.ceil() as i32).max(1);
            if cursor_x >= width as i32 {
                break;
            }
        }
    }
    (pixels, width, height)
}

pub(crate) fn append_sign_text_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    sign: &crate::render::scene::SignEntry,
    region: &crate::render::entity::atlas::MobTextureRegion,
) {
    let (center, normal) = sign_text_plane(sign);
    let right = [normal[2], 0.0, -normal[0]];
    let half_width = 15.0 / 32.0;
    let half_height = 5.0 / 24.0;
    let texel = 0.5 / ENTITY_ATLAS_SIZE as f32;
    let u0 = region.u_min + texel;
    let v0 = region.v_min + texel;
    let u1 = region.u_max - texel;
    let v1 = region.v_max - texel;
    let base = vertices.len() as u32;
    for (position, uv) in [
        (
            [
                center[0] - right[0] * half_width,
                center[1] + half_height,
                center[2] - right[2] * half_width,
            ],
            [u0, v0],
        ),
        (
            [
                center[0] + right[0] * half_width,
                center[1] + half_height,
                center[2] + right[2] * half_width,
            ],
            [u1, v0],
        ),
        (
            [
                center[0] + right[0] * half_width,
                center[1] - half_height,
                center[2] + right[2] * half_width,
            ],
            [u1, v1],
        ),
        (
            [
                center[0] - right[0] * half_width,
                center[1] - half_height,
                center[2] - right[2] * half_width,
            ],
            [u0, v1],
        ),
    ] {
        vertices.push(crate::render::entity::mesh::EntityVertex {
            position,
            normal,
            uv,
            color: [1.0; 4],
        });
    }
    indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

pub(crate) fn append_player_skull_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    skull: &SkullRenderEntry,
    region: &crate::render::entity::atlas::MobTextureRegion,
) {
    // TileEntitySkullRenderer renders a model-space head at a
    // facing-specific anchor. The old path first oriented the bounds and then
    // rotated that oriented box again, moving south/east/west wall skulls.
    let (anchor, yaw) = skull_anchor_and_yaw(skull);
    let texture_width = region.tex_width.max(1) as f32;
    let texture_height = region.tex_height.max(1) as f32;
    let packed_light =
        16.0 + skull.sky_light.min(15) as f32 * 16.0 + skull.block_light.min(15) as f32;
    append_skull_box(
        vertices,
        indices,
        anchor,
        yaw,
        [-0.25, 0.0, -0.25],
        [0.25, 0.5, 0.25],
        skull_box_uvs(0.0, 0.0, texture_width, texture_height),
        region,
        packed_light,
    );

    // ModelHumanoidHead is used by zombie and player skulls in 1.8.9. Its
    // second ModelRenderer is the 0.25-pixel outer head/hat layer at (32, 0).
    if matches!(skull.skull_type, 2 | 3) {
        append_skull_box(
            vertices,
            indices,
            anchor,
            yaw,
            [-0.265625, -0.015625, -0.265625],
            [0.265625, 0.515625, 0.265625],
            skull_box_uvs(32.0, 0.0, texture_width, texture_height),
            region,
            packed_light,
        );
    }
}

fn skull_box_uvs(
    texture_x: f32,
    texture_y: f32,
    texture_width: f32,
    texture_height: f32,
) -> [[f32; 4]; 6] {
    let uv = |x0: f32, y0: f32, x1: f32, y1: f32| {
        [
            x0 / texture_width,
            y0 / texture_height,
            x1 / texture_width,
            y1 / texture_height,
        ]
    };
    [
        uv(
            texture_x + 8.0,
            texture_y + 8.0,
            texture_x + 16.0,
            texture_y + 16.0,
        ),
        uv(
            texture_x + 24.0,
            texture_y + 8.0,
            texture_x + 32.0,
            texture_y + 16.0,
        ),
        uv(
            texture_x + 8.0,
            texture_y,
            texture_x + 16.0,
            texture_y + 8.0,
        ),
        uv(
            texture_x + 16.0,
            texture_y,
            texture_x + 24.0,
            texture_y + 8.0,
        ),
        uv(
            texture_x,
            texture_y + 8.0,
            texture_x + 8.0,
            texture_y + 16.0,
        ),
        uv(
            texture_x + 16.0,
            texture_y + 8.0,
            texture_x + 24.0,
            texture_y + 16.0,
        ),
    ]
}

fn skull_anchor_and_yaw(skull: &SkullRenderEntry) -> ([f32; 3], f32) {
    let [x, y, z] = skull.position;
    let rotation = skull.rotation as f32 * (std::f32::consts::TAU / 16.0);
    match skull.block_metadata & 0x07 {
        // NORTH keeps the block entity's 16-way rotation; the other wall
        // facings use the fixed rotations from TileEntitySkullRenderer.
        2 => ([x as f32 + 0.5, y as f32 + 0.25, z as f32 + 0.74], rotation),
        3 => (
            [x as f32 + 0.5, y as f32 + 0.25, z as f32 + 0.26],
            std::f32::consts::PI,
        ),
        4 => (
            [x as f32 + 0.74, y as f32 + 0.25, z as f32 + 0.5],
            -std::f32::consts::FRAC_PI_2,
        ),
        5 => (
            [x as f32 + 0.26, y as f32 + 0.25, z as f32 + 0.5],
            std::f32::consts::FRAC_PI_2,
        ),
        _ => ([x as f32 + 0.5, y as f32, z as f32 + 0.5], rotation),
    }
}

#[allow(clippy::too_many_arguments)]
fn append_skull_box(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    anchor: [f32; 3],
    yaw: f32,
    min: [f32; 3],
    max: [f32; 3],
    face_uvs: [[f32; 4]; 6],
    region: &MobTextureRegion,
    packed_light: f32,
) {
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    let faces = [
        ([7, 6, 5, 4], [0.0, 0.0, 1.0], 1),
        ([2, 3, 0, 1], [0.0, 0.0, -1.0], 0),
        ([6, 7, 3, 2], [0.0, 1.0, 0.0], 2),
        ([1, 0, 4, 5], [0.0, -1.0, 0.0], 3),
        ([6, 2, 1, 5], [1.0, 0.0, 0.0], 5),
        ([3, 7, 4, 0], [-1.0, 0.0, 0.0], 4),
    ];
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let transform = |point: [f32; 3]| {
        [
            anchor[0] + point[0] * cos_yaw - point[2] * sin_yaw,
            anchor[1] + point[1],
            anchor[2] + point[0] * sin_yaw + point[2] * cos_yaw,
        ]
    };
    let rotate_normal = |normal: [f32; 3]| {
        [
            normal[0] * cos_yaw - normal[2] * sin_yaw,
            normal[1],
            normal[0] * sin_yaw + normal[2] * cos_yaw,
        ]
    };
    let corner_uvs = [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    for (corner_indices, normal, uv_index) in faces {
        let base = vertices.len() as u32;
        let [u0, v0, u1, v1] = face_uvs[uv_index];
        for (corner_index, [local_u, local_v]) in corner_indices.into_iter().zip(corner_uvs) {
            let (u, v) = region.local_to_atlas(u0 + local_u * (u1 - u0), v0 + local_v * (v1 - v0));
            vertices.push(crate::render::entity::mesh::EntityVertex {
                position: transform(corners[corner_index as usize]),
                normal: rotate_normal(normal),
                uv: [u, v],
                color: [1.0, 1.0, 1.0, packed_light],
            });
        }
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

pub(crate) fn append_chest_entity_mesh(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    chest: &ChestRenderEntry,
    region: crate::render::entity::atlas::MobTextureRegion,
) {
    use nalgebra::{Matrix4, Rotation3, Vector3};

    let double = chest.double_x || chest.double_z;
    let source_center = if double { [1.0, 0.5] } else { [0.5, 0.5] };
    let target_center = if chest.double_z {
        [0.5, 1.0]
    } else {
        source_center
    };
    let angle = match chest.metadata & 7 {
        2 => 180.0_f32,
        4 => -90.0,
        5 => 90.0,
        _ => 0.0,
    };
    let [x, y, z] = chest.position;
    let base = Matrix4::new_translation(&Vector3::new(x as f32, y as f32, z as f32))
        * Matrix4::new_translation(&Vector3::new(target_center[0], 0.0, target_center[1]))
        * Rotation3::from_axis_angle(&Vector3::y_axis(), angle.to_radians()).to_homogeneous()
        * Matrix4::new_translation(&Vector3::new(-source_center[0], 0.0, -source_center[1]));
    let eased = 1.0 - (1.0 - chest.lid_angle.clamp(0.0, 1.0)).powi(3);
    let lid = base
        * Matrix4::new_translation(&Vector3::new(0.0, 9.0 / 16.0, 1.0 / 16.0))
        * Rotation3::from_axis_angle(&Vector3::x_axis(), -eased * std::f32::consts::FRAC_PI_2)
            .to_homogeneous()
        * Matrix4::new_translation(&Vector3::new(0.0, -9.0 / 16.0, -1.0 / 16.0));

    let (width, texture_width) = if double { (30.0, 128.0) } else { (14.0, 64.0) };
    append_chest_box(
        vertices,
        indices,
        &base,
        [1.0 / 16.0, 0.0, 1.0 / 16.0],
        [(1.0 + width) / 16.0, 10.0 / 16.0, 15.0 / 16.0],
        [0.0, 19.0],
        [width, 10.0, 14.0],
        [texture_width, 64.0],
        region,
        chest.sky_light,
        chest.block_light,
    );
    append_chest_box(
        vertices,
        indices,
        &lid,
        [1.0 / 16.0, 9.0 / 16.0, 1.0 / 16.0],
        [(1.0 + width) / 16.0, 14.0 / 16.0, 15.0 / 16.0],
        [0.0, 0.0],
        [width, 5.0, 14.0],
        [texture_width, 64.0],
        region,
        chest.sky_light,
        chest.block_light,
    );
    let knob_center = if double { 16.0 } else { 8.0 };
    append_chest_box(
        vertices,
        indices,
        &lid,
        [(knob_center - 1.0) / 16.0, 7.0 / 16.0, 15.0 / 16.0],
        [(knob_center + 1.0) / 16.0, 11.0 / 16.0, 1.0],
        [0.0, 0.0],
        [2.0, 4.0, 1.0],
        [texture_width, 64.0],
        region,
        chest.sky_light,
        chest.block_light,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_chest_box(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    transform: &nalgebra::Matrix4<f32>,
    from: [f32; 3],
    to: [f32; 3],
    texture_offset: [f32; 2],
    dimensions: [f32; 3],
    texture_size: [f32; 2],
    region: crate::render::entity::atlas::MobTextureRegion,
    sky_light: u8,
    block_light: u8,
) {
    use nalgebra::{Vector3, Vector4};

    let [width, height, depth] = dimensions;
    let [texture_x, texture_y] = texture_offset;
    let uv_rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        [
            [x0 / texture_size[0], y0 / texture_size[1]],
            [x1 / texture_size[0], y0 / texture_size[1]],
            [x1 / texture_size[0], y1 / texture_size[1]],
            [x0 / texture_size[0], y1 / texture_size[1]],
        ]
    };
    let faces = [
        (
            [0.0, 1.0, 0.0],
            [
                [from[0], to[1], to[2]],
                [to[0], to[1], to[2]],
                [to[0], to[1], from[2]],
                [from[0], to[1], from[2]],
            ],
            uv_rect(
                texture_x + depth,
                texture_y,
                texture_x + depth + width,
                texture_y + depth,
            ),
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [from[0], from[1], from[2]],
                [to[0], from[1], from[2]],
                [to[0], from[1], to[2]],
                [from[0], from[1], to[2]],
            ],
            uv_rect(
                texture_x + depth + width,
                texture_y,
                texture_x + depth + width * 2.0,
                texture_y + depth,
            ),
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [to[0], from[1], from[2]],
                [from[0], from[1], from[2]],
                [from[0], to[1], from[2]],
                [to[0], to[1], from[2]],
            ],
            uv_rect(
                texture_x + depth * 2.0 + width,
                texture_y + depth,
                texture_x + depth * 2.0 + width * 2.0,
                texture_y + depth + height,
            ),
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [from[0], from[1], to[2]],
                [to[0], from[1], to[2]],
                [to[0], to[1], to[2]],
                [from[0], to[1], to[2]],
            ],
            uv_rect(
                texture_x + depth,
                texture_y + depth,
                texture_x + depth + width,
                texture_y + depth + height,
            ),
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [from[0], from[1], from[2]],
                [from[0], from[1], to[2]],
                [from[0], to[1], to[2]],
                [from[0], to[1], from[2]],
            ],
            uv_rect(
                texture_x,
                texture_y + depth,
                texture_x + depth,
                texture_y + depth + height,
            ),
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [to[0], from[1], to[2]],
                [to[0], from[1], from[2]],
                [to[0], to[1], from[2]],
                [to[0], to[1], to[2]],
            ],
            uv_rect(
                texture_x + depth + width,
                texture_y + depth,
                texture_x + depth * 2.0 + width,
                texture_y + depth + height,
            ),
        ),
    ];
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    let packed_light = 16.0 + sky_light.min(15) as f32 * 16.0 + block_light.min(15) as f32;
    for (normal, points, uvs) in faces {
        let base = vertices.len() as u32;
        let transformed_normal =
            (normal_matrix * Vector3::new(normal[0], normal[1], normal[2])).normalize();
        for (point, uv) in points.into_iter().zip(uvs) {
            let world = transform * Vector4::new(point[0], point[1], point[2], 1.0);
            let (u, v) = region.local_to_atlas(uv[0], uv[1]);
            vertices.push(crate::render::entity::mesh::EntityVertex {
                position: [world.x, world.y, world.z],
                normal: [
                    transformed_normal.x,
                    transformed_normal.y,
                    transformed_normal.z,
                ],
                uv: [u, v],
                color: [1.0, 1.0, 1.0, packed_light],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}
