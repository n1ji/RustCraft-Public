use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{BindGroupHandle, MaterialHandle, PipelineHandle, RhiResult};

use crate::client::player::Camera;
use crate::render::entity::mesh::EntityVertex;
use crate::render::first_person::FirstPersonPose;
use crate::render::hud::HudRenderer;
use crate::render::MeshDraw;

use super::common::*;
use super::items::*;

pub(crate) fn upload_first_person_draw<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pipeline: PipelineHandle,
    bind_group: BindGroupHandle,
    material: MaterialHandle,
    vertices: &[crate::render::entity::mesh::EntityVertex],
    indices: &[u32],
    label: &str,
) -> RhiResult<Option<MeshDraw>> {
    let mut draw = crate::render::mesh::upload_entity_mesh(
        runtime,
        pipeline,
        Some(bind_group),
        vertices,
        indices,
        label,
    )?;
    if let Some(draw) = draw.as_mut() {
        draw.material = Some(material);
    }
    Ok(draw)
}

pub(crate) fn first_person_item_transform(
    camera: &Camera,
    hud: &crate::render::hud::HudRenderer,
    pose: &crate::render::first_person::FirstPersonPose,
) -> nalgebra::Matrix4<f32> {
    first_person_item_transform_with_camera(Some(camera), hud, pose)
}

pub(crate) fn first_person_item_transform_view_space(
    hud: &crate::render::hud::HudRenderer,
    pose: &crate::render::first_person::FirstPersonPose,
) -> nalgebra::Matrix4<f32> {
    first_person_item_transform_with_camera(None, hud, pose)
}

fn first_person_item_transform_with_camera(
    camera: Option<&Camera>,
    hud: &crate::render::hud::HudRenderer,
    pose: &crate::render::first_person::FirstPersonPose,
) -> nalgebra::Matrix4<f32> {
    use nalgebra::{Matrix4, Rotation3, Scale3, Vector3};

    let translate = |x, y, z| Matrix4::new_translation(&Vector3::new(x, y, z));
    let rotate_x = |degrees: f32| {
        Rotation3::from_axis_angle(&Vector3::x_axis(), degrees.to_radians()).to_homogeneous()
    };
    let rotate_y = |degrees: f32| {
        Rotation3::from_axis_angle(&Vector3::y_axis(), degrees.to_radians()).to_homogeneous()
    };
    let rotate_z = |degrees: f32| {
        Rotation3::from_axis_angle(&Vector3::z_axis(), degrees.to_radians()).to_homogeneous()
    };
    let swing = if pose.use_kind == 0 || pose.vanilla_flags.swing {
        pose.swing_progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let sqrt_swing = swing.sqrt();
    let pi = std::f32::consts::PI;
    let swing_x = if pose.vanilla_flags.swing {
        -0.4 * (sqrt_swing * pi).sin()
    } else {
        0.0
    };
    let swing_y = if pose.vanilla_flags.swing {
        0.2 * (sqrt_swing * pi * 2.0).sin()
    } else {
        0.0
    };
    let swing_z = if pose.vanilla_flags.swing {
        -0.2 * (swing * pi).sin()
    } else {
        0.0
    };
    let swing_twist = if pose.vanilla_flags.swing {
        (swing * swing * pi).sin()
    } else {
        0.0
    };
    let swing_arc = if pose.vanilla_flags.swing {
        (sqrt_swing * pi).sin()
    } else {
        0.0
    };

    let mut transform = Matrix4::identity();
    if matches!(pose.use_kind, 2 | 3) && pose.vanilla_flags.use_transform {
        let remaining = (1.0 - pose.use_progress / 1.6).clamp(0.0, 1.0);
        let bob = if remaining < 0.8 {
            ((remaining * 8.0) * pi).cos().abs() * 0.1
        } else {
            0.0
        };
        let mouth = 1.0 - remaining.powi(27);
        transform = transform
            * translate(0.0, bob, 0.0)
            * translate(mouth * 0.6, mouth * -0.5, 0.0)
            * rotate_y(mouth * 90.0)
            * rotate_x(mouth * 10.0)
            * rotate_z(mouth * 30.0);
    }
    transform *= translate(swing_x, swing_y, swing_z);
    transform *= translate(0.56, -0.52, -0.72);
    if pose.vanilla_flags.equip {
        transform *= translate(0.0, pose.equip_progress * -0.6, 0.0);
    }
    transform = transform
        * rotate_y(45.0)
        * rotate_y(swing_twist * -20.0)
        * rotate_z(swing_arc * -20.0)
        * rotate_x(swing_arc * -80.0)
        * Scale3::new(0.4, 0.4, 0.4).to_homogeneous();
    match pose.use_kind {
        1 if pose.vanilla_flags.block_transform => {
            transform = transform
                * translate(-0.5, 0.2, 0.0)
                * rotate_y(30.0)
                * rotate_x(-80.0)
                * rotate_y(60.0);
        }
        4 if pose.vanilla_flags.bow_transform => {
            let charge_ticks = (pose.use_progress * 20.0 - 1.0).clamp(0.0, 72.0);
            let mut pull = charge_ticks / 20.0;
            pull = ((pull * pull + pull * 2.0) / 3.0).clamp(0.0, 1.0);
            let tremble = if pull > 0.1 {
                ((charge_ticks - 0.1) * 1.3).sin() * (pull - 0.1) * 0.01
            } else {
                0.0
            };
            transform = transform
                * rotate_z(-18.0)
                * rotate_y(-12.0)
                * rotate_x(-8.0)
                * translate(-0.9, 0.2 + tremble, pull * 0.1)
                * Scale3::new(1.0, 1.0, 1.0 + pull * 0.2).to_homogeneous();
        }
        _ => {}
    }
    transform = apply_item_camera_transform(
        transform,
        crate::render::item_icons::item_render_info(hud.hand_item_id).first_person,
    );
    transform =
        crate::render::first_person::apply_script_transform(transform, &pose.script_transform);
    if let Some(camera) = camera {
        transform = crate::render::first_person::camera_effect_transform(
            camera,
            pose.render_arm_pitch,
            pose.render_arm_yaw,
        ) * transform;
    }
    // Held geometry is camera-space data. The FirstPerson graph pass owns
    // projection/depth state and does not need camera-world rebaking.
    transform
}

pub(crate) fn apply_item_camera_transform(
    transform: nalgebra::Matrix4<f32>,
    camera: crate::render::item_icons::ItemCameraTransform,
) -> nalgebra::Matrix4<f32> {
    use nalgebra::{Rotation3, Scale3, Vector3};

    transform
        * nalgebra::Matrix4::new_translation(&Vector3::from(camera.translation))
        * Rotation3::from_axis_angle(&Vector3::y_axis(), camera.rotation[1].to_radians())
            .to_homogeneous()
        * Rotation3::from_axis_angle(&Vector3::x_axis(), camera.rotation[0].to_radians())
            .to_homogeneous()
        * Rotation3::from_axis_angle(&Vector3::z_axis(), camera.rotation[2].to_radians())
            .to_homogeneous()
        * Scale3::new(camera.scale[0], camera.scale[1], camera.scale[2]).to_homogeneous()
}

pub(crate) fn append_transformed_block_model(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    block_id: u16,
    damage: u16,
    transform: nalgebra::Matrix4<f32>,
    packed_light: f32,
) -> bool {
    use nalgebra::{Vector3, Vector4};

    if !crate::world::block_models::BlockModelCache::is_available() {
        return append_fallback_block_cube(vertices, indices, block_id, transform, packed_light);
    }
    let cache = crate::world::block_models::BlockModelCache::global();
    let Some(model) = cache.get_model(block_id, damage as u8) else {
        return append_fallback_block_cube(vertices, indices, block_id, transform, packed_light);
    };
    let block = crate::world::block::Block::from_state((block_id << 4) | (damage & 0x0f));
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    let before = indices.len();
    for face in &model.faces {
        if face.vertices.len() < 4 {
            continue;
        }
        let rect = crate::assets::texture::tile_uv_rect(cache.texture_index(&face.texture));
        let material = crate::world::material::model_material(block, face.tintindex);
        let tint = crate::world::material::material_color(material);
        let normal = (normal_matrix * Vector3::from(face.normal)).normalize();
        let base = vertices.len() as u32;
        for index in 0..4 {
            let point = face.vertices[index];
            let world = transform
                * Vector4::new(
                    (point[0] - 8.0) / 16.0,
                    (point[1] - 8.0) / 16.0,
                    (point[2] - 8.0) / 16.0,
                    1.0,
                );
            let local_uv = face.uvs[index];
            vertices.push(crate::render::entity::mesh::EntityVertex {
                position: [world.x, world.y, world.z],
                normal: [normal.x, normal.y, normal.z],
                uv: [
                    rect[0] + local_uv[0] * (rect[2] - rect[0]),
                    rect[1] + local_uv[1] * (rect[3] - rect[1]),
                ],
                color: [tint[0], tint[1], tint[2], packed_light],
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
    indices.len() > before
}

pub(crate) fn append_fallback_block_cube(
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
    block_id: u16,
    transform: nalgebra::Matrix4<f32>,
    packed_light: f32,
) -> bool {
    use nalgebra::{Vector3, Vector4};

    let block = crate::world::block::Block::from_id(block_id);
    if block.to_id() == 0 {
        return false;
    }
    let (top, bottom, side) = block.tiles();
    let faces = [
        (
            [0.0, 1.0, 0.0],
            top,
            [
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            bottom,
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            side,
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            side,
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            side,
            [
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            side,
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
    ];
    let normal_matrix = transform.fixed_view::<3, 3>(0, 0);
    for (normal, tile, points) in faces {
        let rect = crate::assets::texture::tile_uv_rect(tile);
        let normal = (normal_matrix * Vector3::from(normal)).normalize();
        let base = vertices.len() as u32;
        for (point, uv) in points.into_iter().zip([
            [rect[0], rect[3]],
            [rect[2], rect[3]],
            [rect[2], rect[1]],
            [rect[0], rect[1]],
        ]) {
            let world = transform * Vector4::new(point[0], point[1], point[2], 1.0);
            vertices.push(crate::render::entity::mesh::EntityVertex {
                position: [world.x, world.y, world.z],
                normal: [normal.x, normal.y, normal.z],
                uv,
                color: [1.0, 1.0, 1.0, packed_light],
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
    true
}
