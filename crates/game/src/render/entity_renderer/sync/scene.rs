use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::RhiResult;

use super::super::meshes::*;
use super::super::state::EntityRenderer;
use crate::client::player::Camera;

impl EntityRenderer {
    pub(super) fn sync_scene_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        camera: &Camera,
    ) -> RhiResult<()> {
        use std::hash::Hasher;

        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(());
        };

        let mut hasher = fnv::FnvHasher::default();
        hasher.write_u64(self.entity_state_hash);
        hasher.write_u64(self.entity_billboard_generation);
        hasher.write_u64(self.player_skin_content_hash);
        // Remote entities are world-space geometry. Keep only the camera cell
        // for transparent sort updates; camera rotation must not rebuild all
        // remote entity meshes.
        for value in [camera.position.x, camera.position.y, camera.position.z] {
            hasher.write_i32(value.floor() as i32);
        }
        let scene_hash = hasher.finish();
        let local_scene_hash = self
            .local_player_billboard
            .as_ref()
            .map(|local| local_scene_hash(local, self.player_skin_content_hash))
            .unwrap_or(0);
        let scene_dirty = self.uploaded_scene_hash != Some(scene_hash);
        let local_scene_dirty = self.uploaded_local_scene_hash != Some(local_scene_hash);
        if !scene_dirty && !local_scene_dirty {
            return Ok(());
        }

        let atlas = self.entity_atlas.as_ref();

        if scene_dirty {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            let mut item_vertices = Vec::new();
            let mut item_indices = Vec::new();

            for billboard in &self.entity_billboards {
                // The local third-person player is animated independently
                // below. Keeping the sentinel in this batch makes every
                // interpolated redraw rebuild all remote entities.
                if billboard.entity_id == i32::MIN || billboard.invisible {
                    continue;
                }
                if billboard.entity_type == crate::entity::EntityType::Painting {
                    if let Some(region) = atlas.and_then(|atlas| atlas.region_for("painting")) {
                        append_painting_mesh(&mut vertices, &mut indices, billboard, region);
                    }
                    continue;
                }
                if billboard.entity_type == crate::entity::EntityType::ItemFrame {
                    if let Some(atlas) = atlas {
                        if let (Some(wood), Some(background)) = (
                            atlas.region_for("item_frame_wood"),
                            atlas.region_for("item_frame_background"),
                        ) {
                            append_item_frame_mesh(
                                &mut vertices,
                                &mut indices,
                                billboard,
                                wood,
                                background,
                            );
                        }
                    }
                    append_item_frame_display(&mut item_vertices, &mut item_indices, billboard);
                    continue;
                }
                if billboard.kind == crate::render::scene::EntityBillboardKind::Item {
                    append_dropped_item_sprite(&mut item_vertices, &mut item_indices, billboard);
                    continue;
                }
                if billboard.entity_type == crate::entity::EntityType::FishingHook {
                    if let Some(atlas) = atlas {
                        if let (Some(hook), Some(white)) = (
                            atlas.region_for("fishing_hook"),
                            atlas.region_for("__white"),
                        ) {
                            append_fishing_hook_mesh(
                                &mut vertices,
                                &mut indices,
                                billboard,
                                camera,
                                hook,
                                white,
                            );
                        }
                    }
                    continue;
                }
                if let Some((item_id, damage, scale)) = projectile_item(billboard.entity_type) {
                    append_camera_facing_item_sprite(
                        &mut item_vertices,
                        &mut item_indices,
                        billboard.position,
                        item_id,
                        damage,
                        scale,
                        camera,
                        packed_light_for(billboard.sky_light, billboard.block_light),
                    );
                    continue;
                }
                append_model_entity(
                    &mut self.model_cache,
                    atlas,
                    billboard,
                    &mut vertices,
                    &mut indices,
                );
            }

            let new_draw = if vertices.is_empty() || indices.is_empty() {
                None
            } else {
                let mut draw = crate::render::mesh::upload_entity_mesh(
                    runtime,
                    gpu.pipeline,
                    Some(gpu.bind_group),
                    &vertices,
                    &indices,
                    "game.entity.scene",
                )?;
                if let Some(draw) = draw.as_mut() {
                    draw.material = Some(gpu.material);
                }
                draw
            };
            let old_draw = std::mem::replace(&mut self.scene_draw, new_draw);
            if let Some(draw) = old_draw {
                let _ = crate::render::mesh::destroy_mesh(runtime, draw);
            }

            let new_item_draw = if item_vertices.is_empty() || item_indices.is_empty() {
                None
            } else if let (Some(bind_group), Some(material)) =
                (gpu.item_bind_group, gpu.item_material)
            {
                let mut draw = crate::render::mesh::upload_entity_mesh(
                    runtime,
                    gpu.transparent_pipeline,
                    Some(bind_group),
                    &item_vertices,
                    &item_indices,
                    "game.entity.scene-items",
                )?;
                if let Some(draw) = draw.as_mut() {
                    draw.material = Some(material);
                    draw.sort_key = transparent_sort_key(camera, &item_vertices);
                }
                draw
            } else {
                None
            };
            let old_item_draw = std::mem::replace(&mut self.scene_item_draw, new_item_draw);
            if let Some(draw) = old_item_draw {
                let _ = crate::render::mesh::destroy_mesh(runtime, draw);
            }
            self.uploaded_scene_hash = Some(scene_hash);
        }

        if local_scene_dirty {
            let (vertices, indices) = if let Some(local) = self.local_player_billboard.as_ref() {
                if local.invisible {
                    (Vec::new(), Vec::new())
                } else {
                    let mut vertices = Vec::new();
                    let mut indices = Vec::new();
                    append_model_entity(
                        &mut self.model_cache,
                        atlas,
                        local,
                        &mut vertices,
                        &mut indices,
                    );
                    (vertices, indices)
                }
            } else {
                (Vec::new(), Vec::new())
            };
            crate::render::mesh::update_entity_mesh(
                runtime,
                &mut self.local_scene_draw,
                gpu.pipeline,
                Some(gpu.bind_group),
                Some(gpu.material),
                &vertices,
                &indices,
                "game.entity.local-player",
            )?;
            self.uploaded_local_scene_hash = Some(local_scene_hash);
        }
        Ok(())
    }
}

fn append_model_entity(
    model_cache: &mut fnv::FnvHashMap<
        u64,
        std::sync::Arc<Vec<crate::render::entity::mesh::ModelCuboid>>,
    >,
    atlas: Option<&crate::render::entity::atlas::EntityTextureAtlas>,
    billboard: &crate::render::scene::EntityBillboard,
    vertices: &mut Vec<crate::render::entity::mesh::EntityVertex>,
    indices: &mut Vec<u32>,
) {
    let model_key = entity_model_cache_key(billboard);
    let cuboids = std::sync::Arc::clone(model_cache.entry(model_key).or_insert_with(|| {
        std::sync::Arc::new(crate::render::entity::models::model_for_entity(
            billboard.entity_type,
            billboard.visual,
            billboard.slim,
            billboard.skin_parts_mask,
            billboard.has_cape,
        ))
    }));
    if cuboids.is_empty() {
        return;
    }

    let pose = entity_pose(billboard);
    let position = nalgebra::Point3::new(
        billboard.position[0],
        billboard.position[1],
        billboard.position[2],
    );
    let body_cuboids = cuboids
        .iter()
        .filter(|cuboid| cuboid.part_type != crate::render::entity::mesh::PartType::Cape)
        .copied()
        .collect::<Vec<_>>();
    let cape_cuboids = cuboids
        .iter()
        .filter(|cuboid| cuboid.part_type == crate::render::entity::mesh::PartType::Cape)
        .copied()
        .collect::<Vec<_>>();
    let (mut entity_vertices, mut entity_indices) =
        crate::render::entity::mesh::generate_entity_mesh(&body_cuboids, position, &pose);

    let atlas_name = billboard.skin_key.as_deref().unwrap_or_else(|| {
        crate::render::entity::models::atlas_name_for_entity(
            billboard.entity_type,
            billboard.visual,
        )
    });
    if let Some(region) = atlas.and_then(|atlas| {
        atlas.region_for(atlas_name).or_else(|| {
            (billboard.entity_type == crate::entity::EntityType::Player)
                .then(|| atlas.region_for("player"))
                .flatten()
        })
    }) {
        remap_entity_uv(&mut entity_vertices, region);
    }

    if !cape_cuboids.is_empty() {
        let (mut cape_vertices, cape_indices) =
            crate::render::entity::mesh::generate_entity_mesh(&cape_cuboids, position, &pose);
        if let Some(region) = atlas.and_then(|atlas| {
            billboard
                .skin_key
                .as_ref()
                .and_then(|key| atlas.region_for(&format!("cape/{key}")))
                .or_else(|| atlas.region_for("player/cape"))
        }) {
            remap_entity_uv(&mut cape_vertices, region);
        }
        append_entity_mesh(
            &mut entity_vertices,
            &mut entity_indices,
            cape_vertices,
            cape_indices,
        );
    }

    append_player_armor(
        atlas,
        billboard,
        position,
        &pose,
        &mut entity_vertices,
        &mut entity_indices,
    );
    apply_entity_vertex_state(&mut entity_vertices, billboard);
    append_entity_mesh(vertices, indices, entity_vertices, entity_indices);
}

fn local_scene_hash(
    local: &crate::render::scene::EntityBillboard,
    player_skin_content_hash: u64,
) -> u64 {
    use std::hash::Hasher;

    let mut hasher = fnv::FnvHasher::default();
    hasher.write_u64(player_skin_content_hash);
    hasher.write_i32(local.entity_id);
    hasher.write_u64(entity_model_cache_key(local));
    for value in local.position {
        hasher.write_i32((value * 4096.0).round() as i32);
    }
    for value in [
        local.yaw,
        local.pitch,
        local.head_yaw,
        local.limb_swing,
        local.limb_swing_amount,
        local.swing_progress,
        local.swing_alpha,
        local.age_ticks,
    ] {
        hasher.write_u32(value.to_bits());
    }
    for value in local.cape_rotation {
        hasher.write_u32(value.to_bits());
    }
    for equipment in local.equipment {
        match equipment {
            Some((item_id, damage)) => {
                hasher.write_u8(1);
                hasher.write_u16(item_id);
                hasher.write_u16(damage);
            }
            None => hasher.write_u8(0),
        }
    }
    hasher.write_u8(local.skin_parts_mask);
    hasher.write_u8(local.slim as u8);
    hasher.write_u8(local.has_cape as u8);
    hasher.write_u8(local.blocking as u8);
    hasher.write_u8(local.sneaking as u8);
    hasher.write_u8(local.riding as u8);
    hasher.write_u8(local.invisible as u8);
    hasher.finish()
}
