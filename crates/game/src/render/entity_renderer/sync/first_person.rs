use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::RhiResult;

use super::super::meshes::*;
use super::super::state::EntityRenderer;
use crate::render::hud::HudRenderer;
use crate::render::settings::RenderSettings;

impl EntityRenderer {
    pub(super) fn sync_first_person_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        hud: &crate::render::hud::HudRenderer,
        settings: &crate::render::settings::RenderSettings,
    ) -> RhiResult<()> {
        use std::hash::Hasher;

        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(());
        };
        let (Some(first_person_bind_group), Some(first_person_material)) =
            (gpu.first_person_bind_group, gpu.first_person_material)
        else {
            return Ok(());
        };
        let mut hasher = fnv::FnvHasher::default();
        hasher.write_u8(settings.camera_mode);
        hasher.write_u8(hud.chat_open as u8);
        hasher.write_u8(hud.inventory_open as u8);
        hasher.write_u32(hud.health.to_bits());
        hasher.write_u16(hud.hand_item_id);
        hasher.write_u16(hud.hand_item_damage);
        hasher.write_u32(hud.hand_swing_progress.to_bits());
        hasher.write_u32(hud.hand_equip_progress.to_bits());
        hasher.write_u8(hud.hand_use_kind);
        hasher.write_u32(hud.hand_use_progress.to_bits());
        hasher.write_u8(self.local_skin_slim as u8);
        hasher.write_u8(settings.skin_parts);
        for transform in [
            &hud.first_person_arm_transform,
            &hud.first_person_item_transform,
        ] {
            for value in transform.iter() {
                hasher.write_u32(value.to_bits());
            }
        }
        for enabled in [
            hud.fp_vanilla_flags.base,
            hud.fp_vanilla_flags.equip,
            hud.fp_vanilla_flags.swing,
            hud.fp_vanilla_flags.use_transform,
            hud.fp_vanilla_flags.block_transform,
            hud.fp_vanilla_flags.bow_transform,
            hud.fp_vanilla_flags.eat_drink_transform,
            hud.fp_vanilla_flags.bob,
        ] {
            hasher.write_u8(enabled as u8);
        }
        let first_person_hash = hasher.finish();
        if self.uploaded_first_person_hash == Some(first_person_hash) {
            return Ok(());
        }

        let hidden =
            settings.camera_mode != 0 || hud.chat_open || hud.inventory_open || hud.health <= 0.0;
        let mut arm_vertices = Vec::new();
        let mut arm_indices = Vec::new();
        let mut block_vertices = Vec::new();
        let mut block_indices = Vec::new();
        let mut item_vertices = Vec::new();
        let mut item_indices = Vec::new();

        if !hidden {
            let pose = crate::render::first_person::FirstPersonPose {
                swing_progress: hud.hand_swing_progress,
                equip_progress: 1.0 - hud.hand_equip_progress,
                // Camera tracking is supplied by the first-person uniform;
                // the resident mesh itself is camera-independent.
                render_arm_pitch: 0.0,
                render_arm_yaw: 0.0,
                use_kind: hud.hand_use_kind,
                use_progress: hud.hand_use_progress,
                script_transform: if hud.hand_item_id == 0 {
                    hud.first_person_arm_transform
                } else {
                    hud.first_person_item_transform
                },
                vanilla_flags: hud.fp_vanilla_flags.clone(),
                glint: false,
            };
            if hud.hand_item_id == 0 {
                (arm_vertices, arm_indices) =
                    crate::render::entity::player_model::generate_first_person_arm_mesh_view_space(
                        self.local_skin_slim,
                        settings.skin_parts & 0x08 != 0,
                        &pose,
                    );
                if let Some(region) = self
                    .entity_atlas
                    .as_ref()
                    .and_then(|atlas| atlas.region_for("player/local"))
                {
                    remap_entity_uv(&mut arm_vertices, region);
                }
                for vertex in &mut arm_vertices {
                    vertex.color[3] = packed_light_for(15, 0);
                }
            } else {
                let transform = first_person_item_transform_view_space(hud, &pose);
                if hud.hand_item_id <= 255
                    && append_transformed_block_model(
                        &mut block_vertices,
                        &mut block_indices,
                        hud.hand_item_id,
                        hud.hand_item_damage,
                        transform,
                        packed_light_for(15, 0),
                    )
                {
                    // The JSON block model path already emitted the mesh.
                } else {
                    append_transformed_item_sprite(
                        &mut item_vertices,
                        &mut item_indices,
                        hud.hand_item_id,
                        hud.hand_item_damage,
                        transform,
                        packed_light_for(15, 0),
                    );
                }
            }
        }

        let new_arm = upload_first_person_draw(
            runtime,
            gpu.first_person_pipeline,
            first_person_bind_group,
            first_person_material,
            &arm_vertices,
            &arm_indices,
            "game.entity.first-person-arm",
        )?;
        let new_block = if let (Some(bind_group), Some(material)) = (
            gpu.first_person_block_bind_group,
            gpu.first_person_block_material,
        ) {
            match upload_first_person_draw(
                runtime,
                gpu.first_person_pipeline,
                bind_group,
                material,
                &block_vertices,
                &block_indices,
                "game.entity.first-person-block",
            ) {
                Ok(draw) => draw,
                Err(error) => {
                    if let Some(draw) = new_arm {
                        let _ = crate::render::mesh::destroy_mesh(runtime, draw);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        let new_item = if let (Some(bind_group), Some(material)) = (
            gpu.first_person_item_bind_group,
            gpu.first_person_item_material,
        ) {
            match upload_first_person_draw(
                runtime,
                gpu.first_person_pipeline,
                bind_group,
                material,
                &item_vertices,
                &item_indices,
                "game.entity.first-person-item",
            ) {
                Ok(draw) => draw,
                Err(error) => {
                    if let Some(draw) = new_arm {
                        let _ = crate::render::mesh::destroy_mesh(runtime, draw);
                    }
                    if let Some(draw) = new_block {
                        let _ = crate::render::mesh::destroy_mesh(runtime, draw);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };

        for old_draw in [
            self.first_person_arm_draw.take(),
            self.first_person_block_draw.take(),
            self.first_person_item_draw.take(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = crate::render::mesh::destroy_mesh(runtime, old_draw);
        }
        self.first_person_arm_draw = new_arm;
        self.first_person_block_draw = new_block;
        self.first_person_item_draw = new_item;
        for draw in [
            self.first_person_arm_draw.as_mut(),
            self.first_person_block_draw.as_mut(),
            self.first_person_item_draw.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            draw.sort_key = u64::MAX;
        }
        self.uploaded_first_person_hash = Some(first_person_hash);
        Ok(())
    }
}
