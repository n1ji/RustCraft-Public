use renderer_api::core::{RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;

use super::state::EntityRenderer;
use crate::render::submit_draws;

impl EntityRenderer {
    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        submit_draws(runtime, RenderPassKind::Geometry, &self.draws);
        if let Some(draw) = self.scene_draw {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::Geometry));
        }
        if let Some(draw) = self.local_scene_draw {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::Geometry));
        }
        if let Some(draw) = self.scene_item_draw {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::Transparent));
        }
        if let Some(draw) = self.particle_draw {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::Transparent));
        }
        if let Some(draw) = self.block_overlay_draw {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::Transparent));
        }
        if let Some(draw) = self.extras_draw {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::Geometry));
        }
        for draw in [
            self.first_person_arm_draw,
            self.first_person_block_draw,
            self.first_person_item_draw,
        ]
        .into_iter()
        .flatten()
        {
            runtime
                .queue_mut()
                .push(draw.packet(RenderPassKind::FirstPerson));
        }
    }
}
