use renderer_api::core::{RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;

use crate::render::gui_renderer::GuiRenderer;
use crate::render::{submit_draws, MeshDraw};

use super::{HudContext, HudRenderer};
impl HudRenderer {
    /// Build every HUD layer as backend-neutral GUI geometry. Texture-specific
    /// batches are uploaded and submitted by `GuiRenderer` through the RHI.
    pub fn rebuild(
        &mut self,
        gui: &mut GuiRenderer,
        settings: &crate::render::settings::RenderSettings,
        frame: &crate::render::frame::FrameRenderer,
        world: &crate::render::world::WorldRenderer,
        screens: &crate::render::screens::ScreenRenderer,
        entity: &crate::render::entity_renderer::EntityRenderer,
    ) {
        self.hovered_tooltip.clear();
        self.pending_tooltip.clear();
        let mut context = HudContext {
            gui,
            settings,
            frame,
            world,
            screens,
            entity,
        };

        if self.hud_visible {
            self.draw_underwater_overlay(&mut context);
            self.draw_script_hud_before(&mut context);
            self.draw_debug(&mut context);
            // Entity overlays and block selection are submitted by the
            // entity renderer's geometry pass now.  Keep the remaining GUI
            // layers in the same order as the old renderer so the crosshair
            // sits below status/chat/title overlays.
            self.draw_crosshair(&mut context);
            self.draw_potion_effects(&mut context);
            self.draw_hotbar_and_status(&mut context);
            self.draw_chat(&mut context);
            self.draw_titles(&mut context);
            self.draw_scoreboard(&mut context);
            self.draw_player_list(&mut context);
        }

        // Interactive overlays intentionally remain usable when scripts hide
        // the normal HUD, matching the old renderer's behavior.
        self.draw_sign_editor(&mut context);
        self.draw_book_editor(&mut context);
        self.draw_resource_pack_notice(&mut context);
        self.draw_death_screen(&mut context);

        if self.hud_visible {
            self.draw_script_hud(&mut context);
        }
        if self.inventory_open {
            self.draw_inventory(&mut context);
        }
        self.draw_tooltip(&mut context);
        context.gui.refresh_button_hits();
    }

    pub fn set_draws(&mut self, draws: Vec<MeshDraw>) {
        self.draws = draws;
    }

    pub fn draws(&self) -> &[MeshDraw] {
        &self.draws
    }

    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        submit_draws(runtime, RenderPassKind::Gui, &self.draws);
    }
}
