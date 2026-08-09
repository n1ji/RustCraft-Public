use crate::client::inventory::ItemStackView;
use crate::render::gui::widgets::MenuMetrics;
use crate::render::gui_renderer::{
    creative_max_scroll_rows, creative_scrollbar_geometry, creative_tab_entries, CreativeItemEntry,
    GuiRenderer, CREATIVE_TAB_INVENTORY, CREATIVE_TAB_SEARCH,
};
use crate::ui::format::format_text;

use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
impl HudRenderer {
    pub(super) fn draw_potion_effects_inventory(
        &self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
    ) {
        let effects = &self.active_potion_effects;
        if effects.is_empty() {
            return;
        }
        let gs = metrics.gs;
        let font_sz = metrics.font_sz;
        let inv_w = 176.0 * gs;
        let panel_x = (metrics.sw - inv_w) * 0.5;
        let panel_right = panel_x + inv_w;
        let start_y = (metrics.sh - 166.0 * gs) * 0.5;

        let effect_w = 140.0 * gs;
        let effect_h = 32.0 * gs;
        let effect_x = panel_right + 4.0 * gs;
        let row_step = if effects.len() > 5 {
            132.0 / (effects.len() - 1) as f32
        } else {
            33.0
        } * gs;

        for (i, effect) in effects.iter().enumerate() {
            let ey = start_y + i as f32 * row_step;

            context.gui.inventory_batch.add_quad(
                effect_x,
                ey,
                effect_w,
                effect_h,
                0.0,
                166.0 / 256.0,
                140.0 / 256.0,
                32.0 / 256.0,
                [1.0, 1.0, 1.0, 1.0],
            );

            if let Some(icon_idx) = crate::entity::potion_icon_index(effect.effect_id) {
                let icon_col = icon_idx as f32 % 8.0;
                let icon_row = (icon_idx as f32 / 8.0).floor();
                let icon_size = 18.0 * gs;
                context.gui.inventory_batch.add_quad(
                    effect_x + 6.0 * gs,
                    ey + 7.0 * gs,
                    icon_size,
                    icon_size,
                    icon_col * 18.0 / 256.0,
                    (198.0 + icon_row * 18.0) / 256.0,
                    18.0 / 256.0,
                    18.0 / 256.0,
                    [1.0, 1.0, 1.0, 1.0],
                );
            }

            let name_key = crate::entity::potion_effect_name(effect.effect_id);
            let name = context.settings.ui_text.dynamic(name_key);
            let amp_str = crate::entity::potion_amplifier_string(effect.amplifier);
            let display = if amp_str.is_empty() {
                name
            } else {
                format!("{} {}", name, amp_str)
            };
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                effect_x + 28.0 * gs,
                ey + 6.0 * gs,
                &display,
                font_sz * 0.9,
                [1.0, 1.0, 1.0, 1.0],
            );

            let dur = crate::entity::potion_duration_string(effect.duration);
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                effect_x + 28.0 * gs,
                ey + 6.0 * gs + font_sz * 1.0,
                &dur,
                font_sz * 0.7,
                [0.7, 0.7, 0.7, 1.0],
            );
        }
    }
}
