use crate::client::inventory::ItemStackView;
use crate::render::gui::widgets::{draw_button, MenuMetrics};
use crate::ui::format::format_text;

use super::{HudContext, HudRenderer};

impl HudRenderer {
    pub(super) fn draw_underwater_overlay(&self, context: &mut HudContext<'_>) {
        if !context.world.underwater {
            return;
        }
        let width = context.gui.viewport[0].max(1) as f32;
        let height = context.gui.viewport[1].max(1) as f32;
        let u = -context.world.underwater_yaw / 64.0;
        let v = context.world.underwater_pitch / 64.0;
        context.gui.underwater_batch.add_quad(
            0.0,
            0.0,
            width,
            height,
            u,
            v,
            4.0,
            4.0,
            [0.6, 0.6, 0.6, 0.5],
        );
    }

    pub(super) fn draw_crosshair(&self, context: &mut HudContext<'_>) {
        if !self.crosshair_visible || self.chat_open || self.inventory_open {
            return;
        }
        let gs = context.settings.gui_scale.max(1) as f32;
        let cx = context.gui.viewport[0].max(1) as f32 * 0.5;
        let cy = context.gui.viewport[1].max(1) as f32 * 0.5;
        let arm = 6.0 * gs;
        let width = gs.max(1.0);
        context.gui.font_batch.fill_rect(
            cx - arm,
            cy - width * 0.5,
            arm * 2.0,
            width,
            [1.0, 1.0, 1.0, 0.82],
        );
        context.gui.font_batch.fill_rect(
            cx - width * 0.5,
            cy - arm,
            width,
            arm * 2.0,
            [1.0, 1.0, 1.0, 0.82],
        );
    }

    pub(super) fn draw_hotbar_and_status(&mut self, context: &mut HudContext<'_>) {
        let sw = context.gui.viewport[0].max(1) as f32;
        let sh = context.gui.viewport[1].max(1) as f32;
        let gs = context.settings.gui_scale.max(1) as f32;
        let font_size = 9.0 * gs;
        let slot_size = 16.0 * gs;
        let hotbar_width = 182.0 * gs;
        let hotbar_x = (sw - hotbar_width) * 0.5;
        let hotbar_y = sh - 26.0 * gs;

        context
            .gui
            .widget_batch
            .draw_hotbar_bg(hotbar_x, hotbar_y, hotbar_width, 22.0 * gs);
        let selected = self.hotbar_selected.min(8);
        context.gui.widget_batch.draw_hotbar_select(
            hotbar_x + selected as f32 * 20.0 * gs - gs,
            hotbar_y - gs,
            24.0 * gs,
            24.0 * gs,
        );

        for (index, &(item_id, count, damage)) in self.hotbar_slots.iter().enumerate() {
            if item_id == 0 || count == 0 {
                continue;
            }
            let x = hotbar_x + 5.0 * gs + index as f32 * 20.0 * gs;
            let y = hotbar_y + 3.0 * gs;
            let stack = ItemStackView {
                item_id,
                count,
                damage,
                nbt: None,
            };
            super::inventory::draw_item_stack(context.gui, x, y, slot_size, &stack, gs, self.time);
        }

        let show_survival = context.world.gamemode != 1 && context.world.gamemode != 3;
        if show_survival {
            self.draw_survival_status(context, hotbar_x, hotbar_y, hotbar_width, gs);
            let xp_y = hotbar_y - 8.0 * gs;
            context.gui.icons_batch.add_quad(
                hotbar_x,
                xp_y,
                hotbar_width,
                5.0 * gs,
                0.0,
                64.0 / 256.0,
                182.0 / 256.0,
                5.0 / 256.0,
                [1.0; 4],
            );
            let progress = self.experience_bar.clamp(0.0, 1.0);
            if progress > 0.0 {
                context.gui.icons_batch.add_quad(
                    hotbar_x,
                    xp_y,
                    hotbar_width * progress,
                    5.0 * gs,
                    0.0,
                    69.0 / 256.0,
                    182.0 / 256.0 * progress,
                    5.0 / 256.0,
                    [1.0; 4],
                );
            }
            if self.experience_level > 0 {
                context.gui.font_batch.draw_text_shadowed(
                    &mut context.gui.font,
                    sw * 0.5,
                    hotbar_y - 21.0 * gs,
                    &self.experience_level.to_string(),
                    font_size * 0.85,
                    [1.0, 1.0, 1.0, 1.0],
                    gs,
                );
            }
        }

        if let Some(text) = self.action_bar.as_deref() {
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                sw * 0.5,
                hotbar_y - 37.0 * gs,
                text,
                font_size * 0.85,
                [1.0, 1.0, 1.0, 0.95],
            );
        }

        if context.world.raining {
            let key = if context.world.thunder_level > 0.05 {
                "rustcraft.weather.thunderstorm"
            } else {
                "rustcraft.weather.rain"
            };
            context.gui.font_batch.fill_rect(
                8.0 * gs,
                sh - 42.0 * gs,
                90.0 * gs,
                12.0 * gs,
                [0.0, 0.0, 0.0, 0.38],
            );
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                12.0 * gs,
                sh - 39.0 * gs,
                context.settings.ui_text.get(key),
                font_size * 0.62,
                [0.62, 0.76, 0.95, 0.9],
            );
        }
    }

    fn draw_survival_status(
        &self,
        context: &mut HudContext<'_>,
        hotbar_x: f32,
        hotbar_y: f32,
        hotbar_width: f32,
        gs: f32,
    ) {
        let status_y = hotbar_y - 20.0 * gs;
        let icon = 9.0 * gs;
        let step = 8.0 * gs;
        let has_absorption = self.absorption > 0.0;
        let has_armor = self.armor_points > 0;
        let absorption_y = if has_absorption {
            status_y - 10.0 * gs
        } else {
            status_y
        };
        let armor_y = if has_armor {
            status_y - (if has_absorption { 20.0 } else { 10.0 }) * gs
        } else {
            status_y
        };

        let mut icon_quad = |x: f32, y: f32, u: f32, v: f32| {
            context.gui.icons_batch.add_quad(
                x,
                y,
                icon,
                icon,
                u / 256.0,
                v / 256.0,
                9.0 / 256.0,
                9.0 / 256.0,
                [1.0; 4],
            );
        };

        if has_absorption {
            let full = (self.absorption * 0.5).floor().clamp(0.0, 10.0) as usize;
            for index in 0..full {
                icon_quad(hotbar_x + index as f32 * step, absorption_y, 160.0, 0.0);
            }
            if self.absorption % 2.0 >= 0.5 && full < 10 {
                icon_quad(hotbar_x + full as f32 * step, absorption_y, 169.0, 0.0);
            }
        }

        let flash = self.health_timer > 0 && (self.health_timer / 3) % 2 == 1;
        let health = if flash && self.prev_health > 0.0 {
            self.prev_health
        } else {
            self.health
        }
        .clamp(0.0, 20.0);
        for index in 0..10 {
            icon_quad(hotbar_x + index as f32 * step, status_y, 16.0, 0.0);
        }
        let full = (health * 0.5).floor() as usize;
        for index in 0..full {
            icon_quad(hotbar_x + index as f32 * step, status_y, 52.0, 0.0);
        }
        if health % 2.0 >= 0.5 && full < 10 {
            icon_quad(hotbar_x + full as f32 * step, status_y, 61.0, 0.0);
        }

        if has_armor {
            let points = self.armor_points.min(20) as usize;
            for index in 0..10 {
                icon_quad(hotbar_x + index as f32 * step, armor_y, 16.0, 9.0);
            }
            for index in 0..points / 2 {
                icon_quad(hotbar_x + index as f32 * step, armor_y, 34.0, 9.0);
            }
            if points % 2 != 0 {
                icon_quad(hotbar_x + (points / 2) as f32 * step, armor_y, 25.0, 9.0);
            }
        }

        let flash = self.food_timer > 0 && (self.food_timer / 3) % 2 == 1;
        let food = if flash && self.prev_food > 0 {
            self.prev_food
        } else {
            self.food
        }
        .clamp(0, 20) as usize;
        let actual_food = self.food.clamp(0, 20) as usize;
        for index in 0..10 {
            let x = hotbar_x + hotbar_width - (index as f32 + 1.0) * step;
            icon_quad(x, status_y, 16.0, 27.0);
        }
        for index in 0..food / 2 {
            let x = hotbar_x + hotbar_width - (index as f32 + 1.0) * step;
            icon_quad(x, status_y, 52.0, 27.0);
        }
        if actual_food % 2 != 0 {
            let x = hotbar_x + hotbar_width - (food / 2) as f32 * step - step;
            icon_quad(x, status_y, 61.0, 27.0);
        }
    }

    pub(super) fn draw_potion_effects(&self, context: &mut HudContext<'_>) {
        if self.active_potion_effects.is_empty() {
            return;
        }
        let gs = context.settings.gui_scale.max(1) as f32;
        let sw = context.gui.viewport[0].max(1) as f32;
        let sh = context.gui.viewport[1].max(1) as f32;
        let size = 18.0 * gs;
        let step = 20.0 * gs;
        let max_visible = ((sh - 4.0 * gs) / step).max(0.0) as usize;
        for (row, icon) in self
            .active_potion_effects
            .iter()
            .filter_map(|effect| crate::entity::potion_icon_index(effect.effect_id))
            .take(max_visible)
            .enumerate()
        {
            let column = icon % 8;
            let atlas_row = icon / 8;
            context.gui.inventory_batch.add_quad(
                sw - size - 2.0 * gs,
                2.0 * gs + row as f32 * step,
                size,
                size,
                column as f32 * 18.0 / 256.0,
                (198.0 + atlas_row as f32 * 18.0) / 256.0,
                18.0 / 256.0,
                18.0 / 256.0,
                [1.0; 4],
            );
        }
    }

    pub(super) fn draw_resource_pack_notice(&self, context: &mut HudContext<'_>) {
        let Some(status) = context.screens.resource_pack_status.as_deref() else {
            return;
        };
        if self.chat_open || self.inventory_open || self.health <= 0.0 {
            return;
        }
        let metrics = hud_metrics(context);
        let sw = metrics.sw;
        let sh = metrics.sh;
        let gs = metrics.gs;
        if status.starts_with("available") {
            let width = 260.0 * gs;
            let height = 92.0 * gs;
            let x = (sw - width) * 0.5;
            let y = (sh - height) * 0.35;
            context
                .gui
                .overlay_batch
                .fill_rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.35]);
            context
                .gui
                .overlay_batch
                .fill_rect(x, y, width, height, [0.0, 0.0, 0.0, 0.82]);
            context
                .gui
                .overlay_batch
                .fill_rect(x, y, width, 1.0 * gs, [0.65, 0.65, 0.65, 0.9]);
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                sw * 0.5,
                y + 10.0 * gs,
                context.settings.ui_text.get("resourcePack.title"),
                metrics.font_sz,
                [1.0; 4],
            );
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                sw * 0.5,
                y + 32.0 * gs,
                context
                    .settings
                    .ui_text
                    .get("rustcraft.resourcePack.prompt"),
                metrics.font_sz * 0.72,
                [0.82, 0.82, 0.82, 1.0],
            );
            let button_width = 96.0 * gs;
            let button_y = y + 58.0 * gs;
            let gui = &mut *context.gui;
            draw_button(
                &metrics,
                &mut gui.widget_batch,
                &mut gui.font_batch,
                crate::ui::button_ids::RESOURCE_PACK_ACCEPT,
                [
                    sw * 0.5 - button_width - 4.0 * gs,
                    button_y,
                    button_width,
                    metrics.btn_h,
                ],
                context
                    .settings
                    .ui_text
                    .get("rustcraft.resourcePack.accept"),
                &mut gui.font,
            );
            draw_button(
                &metrics,
                &mut gui.widget_batch,
                &mut gui.font_batch,
                crate::ui::button_ids::RESOURCE_PACK_DECLINE,
                [sw * 0.5 + 4.0 * gs, button_y, button_width, metrics.btn_h],
                context
                    .settings
                    .ui_text
                    .get("rustcraft.resourcePack.decline"),
                &mut gui.font,
            );
        } else {
            let width = 210.0 * gs;
            let x = sw - width - 8.0 * gs;
            let y = 8.0 * gs;
            context
                .gui
                .font_batch
                .fill_rect(x, y, width, 30.0 * gs, [0.0, 0.0, 0.0, 0.55]);
            context
                .gui
                .font_batch
                .fill_rect(x, y, width, 1.0 * gs, [0.55, 0.55, 0.55, 0.75]);
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                x + 6.0 * gs,
                y + 5.0 * gs,
                context
                    .settings
                    .ui_text
                    .get("rustcraft.resourcePack.status"),
                metrics.font_sz * 0.72,
                [0.95, 0.95, 0.95, 1.0],
            );
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                x + 6.0 * gs,
                y + 17.0 * gs,
                status,
                metrics.font_sz * 0.58,
                [0.7, 0.82, 0.95, 1.0],
            );
        }
    }

    pub(super) fn draw_death_screen(&self, context: &mut HudContext<'_>) {
        if self.health > 0.0 {
            return;
        }
        let metrics = hud_metrics(context);
        let gs = metrics.gs;
        context.gui.overlay_batch.fill_rect_gradient(
            0.0,
            0.0,
            metrics.sw,
            metrics.sh,
            [80.0 / 255.0, 0.0, 0.0, 96.0 / 255.0],
            [128.0 / 255.0, 48.0 / 255.0, 48.0 / 255.0, 160.0 / 255.0],
        );
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            metrics.sw * 0.5,
            metrics.sh * 0.25,
            context.settings.ui_text.get("deathScreen.title"),
            22.0 * gs,
            [1.0; 4],
        );
        let score = self.experience_total.max(0).to_string();
        let score_text = format_text(context.settings.ui_text.get("deathScreen.score"), &[&score]);
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            metrics.sw * 0.5,
            metrics.sh * 0.25 + 24.0 * gs,
            &score_text,
            metrics.font_sz,
            [1.0; 4],
        );
        let gui = &mut *context.gui;
        draw_button(
            &metrics,
            &mut gui.widget_batch,
            &mut gui.font_batch,
            crate::ui::button_ids::RESPAWN,
            [
                metrics.btn_x,
                metrics.sh * 0.25 + 72.0 * gs,
                metrics.btn_w,
                metrics.btn_h,
            ],
            context.settings.ui_text.get("deathScreen.respawn"),
            &mut gui.font,
        );
        draw_button(
            &metrics,
            &mut gui.widget_batch,
            &mut gui.font_batch,
            crate::ui::button_ids::DEATH_TITLE_SCREEN,
            [
                metrics.btn_x,
                metrics.sh * 0.25 + 100.0 * gs,
                metrics.btn_w,
                metrics.btn_h,
            ],
            context.settings.ui_text.get("deathScreen.titleScreen"),
            &mut gui.font,
        );
    }
}

pub(super) fn hud_metrics(context: &HudContext<'_>) -> MenuMetrics {
    MenuMetrics::new(
        context.gui.viewport[0].max(1) as f32,
        context.gui.viewport[1].max(1) as f32,
        context.settings.gui_scale.max(1),
        [context.gui.mouse_x, context.gui.mouse_y],
    )
}
