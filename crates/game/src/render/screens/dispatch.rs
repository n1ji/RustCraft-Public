use renderer_api::core::{RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;

use crate::render::gui::widgets::MenuMetrics;
use crate::render::gui_renderer::GuiRenderer;
use crate::render::settings::RenderSettings;

use super::{ScreenContext, ScreenRenderer};

impl ScreenRenderer {
    pub fn rebuild(
        &mut self,
        menu: u32,
        in_world: bool,
        settings: &RenderSettings,
        gui: &mut GuiRenderer,
    ) {
        let width = gui.viewport[0].max(1) as f32;
        let height = gui.viewport[1].max(1) as f32;
        let scale = settings.gui_scale.max(1);
        gui.clear_batches();

        match (menu, in_world) {
            (0 | 1, _) => {}
            (7 | 13, true) => gui.background_batch.add_quad(
                0.0,
                0.0,
                width,
                height,
                0.0,
                0.0,
                width / (32.0 * scale as f32),
                height / (32.0 * scale as f32),
                [0.25, 0.25, 0.25, 1.0],
            ),
            (_, true) => gui.overlay_batch.fill_world_background(width, height),
            (_, false) => gui.background_batch.add_quad(
                0.0,
                0.0,
                width,
                height,
                0.0,
                0.0,
                width / (32.0 * scale as f32),
                height / (32.0 * scale as f32),
                [0.25, 0.25, 0.25, 1.0],
            ),
        }

        let metrics = MenuMetrics::new(width, height, scale, [gui.mouse_x, gui.mouse_y]);
        let mut context = ScreenContext {
            metrics,
            in_world,
            settings,
            gui,
        };

        if self.shader_pack_loading {
            self.draw_shader_pack_loading(&mut context);
        } else {
            match menu {
                0 => self.draw_main_menu(&mut context),
                1 => {}
                2 => self.draw_pause_menu(&mut context),
                3 => self.draw_options_menu(&mut context),
                4 => self.draw_multiplayer_menu(&mut context),
                5 => self.draw_direct_connect(&mut context),
                6 => self.draw_video_settings(&mut context),
                7 => self.draw_controls(&mut context),
                8 => self.draw_language(&mut context),
                9 => self.draw_audio_settings(&mut context),
                10 => self.draw_skin_customization(&mut context),
                11 => self.draw_connecting(&mut context),
                12 => self.draw_loading_world(&mut context),
                13 => self.draw_resource_packs(&mut context),
                14 => self.draw_disconnected(&mut context),
                15 => self.draw_server_editor(&mut context),
                16 => self.draw_alt_manager(&mut context),
                17 => self.draw_modding(&mut context),
                18 => self.draw_mod_config(&mut context),
                19 => self.draw_shader_packs(&mut context),
                20 => self.draw_chat_settings(&mut context),
                21 => self.draw_shader_options(&mut context),
                _ => {}
            }
        }

        context.gui.refresh_button_hits();
    }

    pub fn set_draws(&mut self, draws: Vec<super::super::MeshDraw>) {
        self.draws = draws;
    }

    pub fn draws(&self) -> &[super::super::MeshDraw] {
        &self.draws
    }

    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        super::super::submit_draws(runtime, RenderPassKind::Gui, &self.draws);
    }
}

pub(crate) fn visible_range(
    item_count: usize,
    first: &mut usize,
    visible: usize,
) -> std::ops::Range<usize> {
    let visible = visible.max(1);
    *first = (*first).min(item_count.saturating_sub(visible));
    *first..(*first + visible).min(item_count)
}
