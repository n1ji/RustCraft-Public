use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::RhiResult;

use super::super::meshes::*;
use super::super::state::EntityRenderer;
use crate::client::interaction::SelectionBox;
use crate::render::entity::atlas::MobTextureRegion;
use crate::render::entity::mesh::EntityVertex;

impl EntityRenderer {
    pub(super) fn sync_block_overlay_mesh<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
    ) -> RhiResult<()> {
        use std::hash::Hasher;

        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(());
        };
        let mut hasher = fnv::FnvHasher::default();
        hasher.write_usize(self.block_selection_boxes.len());
        for selection in &self.block_selection_boxes {
            for value in selection.min.into_iter().chain(selection.max) {
                hasher.write_u32(value.to_bits());
            }
        }
        hasher.write_u32(self.dig_progress.to_bits());
        if let Some(position) = self.dig_position {
            for value in position {
                hasher.write_i32(value);
            }
        }
        let overlay_hash = hasher.finish();
        if self.uploaded_block_overlay_hash == Some(overlay_hash) {
            return Ok(());
        }

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        if let Some(atlas) = self.entity_atlas.as_ref() {
            if let Some(white) = atlas.region_for("__white") {
                append_selection_wireframes(
                    &mut vertices,
                    &mut indices,
                    &self.block_selection_boxes,
                    white,
                );
            }
            if self.dig_progress > 0.0 {
                let stage = (self.dig_progress.clamp(0.0, 1.0) * 10.0).floor().min(9.0) as u32;
                if let Some(region) = atlas.region_for(&format!("destroy_{stage}")) {
                    append_dig_overlay(
                        &mut vertices,
                        &mut indices,
                        &self.block_selection_boxes,
                        self.dig_position,
                        region,
                    );
                }
            }
        }

        let old_draw = self.block_overlay_draw.take();
        let mut new_draw = crate::render::mesh::upload_entity_mesh(
            runtime,
            gpu.transparent_pipeline,
            Some(gpu.transparent_bind_group),
            &vertices,
            &indices,
            "game.entity.block-overlay",
        )?;
        if let Some(draw) = new_draw.as_mut() {
            draw.material = Some(gpu.transparent_material);
            // Selection/dig overlays are camera-facing interaction feedback
            // and must be composited after world transparency.
            draw.sort_key = u64::MAX;
        }
        if let Some(draw) = old_draw {
            let _ = crate::render::mesh::destroy_mesh(runtime, draw);
        }
        self.block_overlay_draw = new_draw;
        self.uploaded_block_overlay_hash = Some(overlay_hash);
        Ok(())
    }
}
