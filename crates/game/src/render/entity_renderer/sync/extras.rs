use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{Extent2D, RhiResult};

use super::super::meshes::*;
use super::super::state::EntityRenderer;
use crate::client::player::Camera;
use crate::render::entity::atlas::ENTITY_ATLAS_SIZE;
use crate::render::scene::SignEntry;
use crate::ui::font::FontRenderer;

impl EntityRenderer {
    pub(super) fn sync_extras_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        font: &mut crate::ui::font::FontRenderer,
        camera: &Camera,
    ) -> RhiResult<()> {
        use std::hash::Hasher;

        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(());
        };
        // TileEntitySignRenderer only draws text within 64 blocks. Filtering
        // before dynamic-atlas packing also prevents distant signs from
        // exhausting the finite runtime text band.
        let visible_signs = self
            .sign_entries
            .iter()
            .filter(|sign| {
                let dx = sign.position[0] as f32 + 0.5 - camera.position.x;
                let dy = sign.position[1] as f32 + 0.5 - camera.position.y;
                let dz = sign.position[2] as f32 + 0.5 - camera.position.z;
                dx * dx + dy * dy + dz * dz <= 64.0 * 64.0
            })
            .collect::<Vec<_>>();
        let mut sign_hasher = fnv::FnvHasher::default();
        for sign in &visible_signs {
            for value in sign.position {
                sign_hasher.write_i32(value);
            }
            sign_hasher.write_u8(sign.wall_mounted as u8);
            sign_hasher.write_u8(sign.metadata);
            for line in &sign.lines {
                sign_hasher.write(line.as_bytes());
                sign_hasher.write_u8(0);
            }
        }
        let sign_hash = sign_hasher.finish();
        if self.uploaded_sign_atlas_hash != Some(sign_hash) {
            if let (Some(atlas), Some(texture)) = (self.entity_atlas.as_mut(), self.entity_texture)
            {
                atlas.clear_sign_texts();
                for sign in &visible_signs {
                    let key = sign_texture_key(sign.position);
                    let (pixels, width, height) = build_sign_texture(font, &sign.lines);
                    if atlas.pack_sign_text(&key, &pixels, width, height).is_none() {
                        log::warn!("entity texture atlas has no room for sign text '{key}'");
                    }
                }
                runtime.device().upload_texture_region(
                    texture,
                    [0, 0],
                    Extent2D::new(ENTITY_ATLAS_SIZE, ENTITY_ATLAS_SIZE),
                    &atlas.pixels,
                    ENTITY_ATLAS_SIZE * 4,
                )?;
            }
            self.uploaded_sign_atlas_hash = Some(sign_hash);
        }

        let mut hasher = fnv::FnvHasher::default();
        hasher.write_u64(sign_hash);
        hasher.write_u64(self.player_skin_content_hash);
        for skull in &self.skull_entries {
            for value in skull.position {
                hasher.write_i32(value);
            }
            hasher.write_u8(skull.block_metadata);
            hasher.write_u8(skull.skull_type);
            hasher.write_u8(skull.rotation);
            hasher.write_u8(skull.sky_light);
            hasher.write_u8(skull.block_light);
            hasher.write(skull.skin_key.as_bytes());
        }
        for chest in &self.chest_entries {
            for value in chest.position {
                hasher.write_i32(value);
            }
            hasher.write_u16(chest.block.to_id());
            hasher.write_u8(chest.metadata);
            hasher.write_u32(chest.lid_angle.to_bits());
            hasher.write_u8(chest.double_x as u8);
            hasher.write_u8(chest.double_z as u8);
            hasher.write_u8(chest.sky_light);
            hasher.write_u8(chest.block_light);
        }
        let extras_hash = hasher.finish();
        if self.uploaded_extras_hash == Some(extras_hash) {
            return Ok(());
        }

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        if let Some(atlas) = self.entity_atlas.as_ref() {
            for skull in &self.skull_entries {
                if let Some(region) = atlas.region_for(&skull.skin_key) {
                    append_player_skull_mesh(&mut vertices, &mut indices, skull, region);
                }
            }
            for chest in &self.chest_entries {
                let variant = match chest.block {
                    crate::world::block::Block::TrappedChest => "trapped",
                    crate::world::block::Block::EnderChest => "ender",
                    _ => "normal",
                };
                let texture = if chest.double_x || chest.double_z {
                    format!("chest_{variant}_double")
                } else {
                    format!("chest_{variant}")
                };
                if let Some(region) = atlas.region_for(&texture).copied() {
                    append_chest_entity_mesh(&mut vertices, &mut indices, chest, region);
                }
            }
            for sign in &visible_signs {
                if let Some(region) = atlas.region_for(&sign_texture_key(sign.position)) {
                    append_sign_text_mesh(&mut vertices, &mut indices, sign, region);
                }
            }
        }

        let old_draw = self.extras_draw.take();
        let mut new_draw = crate::render::mesh::upload_entity_mesh(
            runtime,
            gpu.pipeline,
            Some(gpu.bind_group),
            &vertices,
            &indices,
            "game.entity.extras",
        )?;
        if let Some(draw) = new_draw.as_mut() {
            draw.material = Some(gpu.material);
        }
        if let Some(draw) = old_draw {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        self.extras_draw = new_draw;
        self.uploaded_extras_hash = Some(extras_hash);
        Ok(())
    }
}
