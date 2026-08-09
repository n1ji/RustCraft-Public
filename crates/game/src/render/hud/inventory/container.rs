use crate::client::inventory::ItemStackView;
use crate::render::gui::widgets::MenuMetrics;
use crate::render::gui_renderer::{
    creative_max_scroll_rows, creative_scrollbar_geometry, creative_tab_entries, CreativeItemEntry,
    GuiRenderer, CREATIVE_TAB_INVENTORY, CREATIVE_TAB_SEARCH,
};
use crate::ui::format::format_text;

use super::{
    draw_atlas_sprite, draw_durability_bar, draw_item_stack, window_property, AtlasRegion,
    ContainerKind, ContainerLayout,
};
use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};

impl HudRenderer {
    pub(super) fn draw_container_window(
        &mut self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
    ) {
        let gs = metrics.gs;
        let font_sz = metrics.font_sz;
        let slot_size = 16.0 * gs;
        let slot_gap = 2.0 * gs;
        let layout = ContainerLayout::for_window(
            &self.inventory_window_type,
            self.inventory_window_slot_count,
        );
        let use_generic54 = layout.kind == ContainerKind::Generic;
        let generic_rows = layout.rows.clamp(1, 6);
        let panel_w = if use_generic54 {
            176.0 * gs
        } else {
            layout.atlas_region().w * gs
        };
        let panel_h = if use_generic54 {
            (114.0 + generic_rows as f32 * 18.0) * gs
        } else {
            layout.atlas_region().h * gs
        };
        let panel_x = (metrics.sw - panel_w) * 0.5;
        let panel_y = (metrics.sh - panel_h) * 0.5;
        let player_grid_x = panel_x + layout.player_inventory_x() * gs;
        let container_x = if use_generic54 {
            player_grid_x
        } else {
            panel_x
        };
        let container_y = if use_generic54 {
            panel_y + 18.0 * gs
        } else {
            panel_y
        };
        let main_y = if use_generic54 {
            panel_y + panel_h - 82.0 * gs
        } else {
            panel_y + layout.player_inventory_y() * gs
        };

        if use_generic54 {
            let top_h = generic_rows as f32 * 18.0 + 17.0;
            context.gui.generic54_batch.add_quad(
                panel_x,
                panel_y,
                panel_w,
                top_h * gs,
                0.0,
                0.0,
                176.0 / 1024.0,
                top_h / 768.0,
                [1.0, 1.0, 1.0, 1.0],
            );
            context.gui.generic54_batch.add_quad(
                panel_x,
                panel_y + top_h * gs,
                panel_w,
                96.0 * gs,
                0.0,
                126.0 / 768.0,
                176.0 / 1024.0,
                96.0 / 768.0,
                [1.0, 1.0, 1.0, 1.0],
            );
        } else {
            let atlas = layout.atlas_region();
            context.gui.generic54_batch.add_quad(
                panel_x,
                panel_y,
                atlas.w * gs,
                atlas.h * gs,
                atlas.u(),
                atlas.v(),
                atlas.w / 1024.0,
                atlas.h / 768.0,
                [1.0, 1.0, 1.0, 1.0],
            );
            if layout.kind == ContainerKind::Horse && layout.slot_count > 2 {
                draw_atlas_sprite(
                    &mut context.gui.generic54_batch,
                    panel_x + 79.0 * gs,
                    panel_y + 17.0 * gs,
                    90.0 * gs,
                    54.0 * gs,
                    atlas.cell,
                    0.0,
                    166.0,
                    90.0,
                    54.0,
                );
            }
        }
        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            player_grid_x,
            panel_y + 6.0 * gs,
            &self.inventory_window_title,
            font_sz * 0.64,
            [0.25, 0.25, 0.25, 1.0],
        );

        if layout.kind == ContainerKind::Furnace {
            self.draw_furnace_indicators(context, panel_x, panel_y, gs);
        } else if layout.kind == ContainerKind::BrewingStand {
            self.draw_brewing_indicators(context, panel_x, panel_y, gs);
        } else if layout.kind == ContainerKind::EnchantingTable {
            self.draw_enchanting_indicators(context, panel_x, panel_y, font_sz, gs);
        } else if layout.kind == ContainerKind::Anvil {
            self.draw_anvil_indicators(context, panel_x, panel_y, font_sz, gs);
        }

        for slot in 0..layout.slot_count.min(self.inventory_window_slot_count) {
            let Some((sx, sy)) =
                layout.slot_pos(slot, container_x, container_y, slot_size, slot_gap, gs)
            else {
                continue;
            };
            let item = self
                .inventory_window_slots
                .get(slot)
                .cloned()
                .unwrap_or_default();
            self.draw_inventory_slot(
                context,
                sx,
                sy,
                slot_size,
                item,
                font_sz,
                gs,
                Some(slot as i16),
            );
        }

        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            player_grid_x,
            main_y - 12.0 * gs,
            context.settings.ui_text.get("container.inventory"),
            font_sz * 0.64,
            [0.25, 0.25, 0.25, 1.0],
        );
        for row in 0..3 {
            for col in 0..9 {
                let local_idx = 9 + row * 9 + col;
                let protocol_slot =
                    self.inventory_window_slot_count as i16 + row as i16 * 9 + col as i16;
                let sx = player_grid_x + col as f32 * (slot_size + slot_gap);
                let sy = main_y + row as f32 * (slot_size + slot_gap);
                self.draw_inventory_slot(
                    context,
                    sx,
                    sy,
                    slot_size,
                    self.inventory_slots[local_idx].clone(),
                    font_sz,
                    gs,
                    Some(protocol_slot),
                );
            }
        }

        let hotbar_y = main_y + 58.0 * gs;
        for col in 0..9 {
            let sx = player_grid_x + col as f32 * (slot_size + slot_gap);
            let protocol_slot = self.inventory_window_slot_count as i16 + 27 + col as i16;

            self.draw_inventory_slot(
                context,
                sx,
                hotbar_y,
                slot_size,
                self.inventory_slots[col].clone(),
                font_sz,
                gs,
                Some(protocol_slot),
            );
        }

        self.draw_cursor_stack(context, metrics, slot_size, font_sz, gs);
        self.finish_tooltip(context, metrics, font_sz, gs);
    }

    fn draw_furnace_indicators(&self, context: &mut HudContext<'_>, x: f32, y: f32, gs: f32) {
        let burn = window_property(&self.inventory_window_properties, 0).max(0) as f32;
        let mut burn_total = window_property(&self.inventory_window_properties, 1) as f32;
        if burn_total <= 0.0 {
            burn_total = 200.0;
        }
        let cook = window_property(&self.inventory_window_properties, 2).max(0) as f32;
        let cook_total = window_property(&self.inventory_window_properties, 3).max(1) as f32;
        let burn_h = (13.0 * (burn / burn_total).clamp(0.0, 1.0)) as i32;
        if burn_h > 0 {
            draw_atlas_sprite(
                &mut context.gui.generic54_batch,
                x + 56.0 * gs,
                y + (36.0 + 12.0 - burn_h as f32) * gs,
                14.0 * gs,
                (burn_h + 1) as f32 * gs,
                2,
                176.0,
                12.0 - burn_h as f32,
                14.0,
                (burn_h + 1) as f32,
            );
        }
        let cook_w = (24.0 * (cook / cook_total).clamp(0.0, 1.0)) as i32;
        if cook_w > 0 {
            draw_atlas_sprite(
                &mut context.gui.generic54_batch,
                x + 79.0 * gs,
                y + 34.0 * gs,
                (cook_w + 1) as f32 * gs,
                16.0 * gs,
                2,
                176.0,
                14.0,
                (cook_w + 1) as f32,
                16.0,
            );
        }
    }

    fn draw_brewing_indicators(&self, context: &mut HudContext<'_>, x: f32, y: f32, gs: f32) {
        let brew = window_property(&self.inventory_window_properties, 0).max(0) as f32;
        if brew <= 0.0 {
            return;
        }
        let progress_h = (28.0f32 * (1.0f32 - brew / 400.0f32)).max(0.0f32) as i32;
        if progress_h > 0 {
            draw_atlas_sprite(
                &mut context.gui.generic54_batch,
                x + 97.0 * gs,
                y + 16.0 * gs,
                9.0 * gs,
                progress_h as f32 * gs,
                5,
                176.0,
                0.0,
                9.0,
                progress_h as f32,
            );
        }
        let bubble_h = match (brew as i32 / 2) % 7 {
            0 => 29,
            1 => 24,
            2 => 20,
            3 => 16,
            4 => 11,
            5 => 6,
            _ => 0,
        };
        if bubble_h > 0 {
            draw_atlas_sprite(
                &mut context.gui.generic54_batch,
                x + 65.0 * gs,
                y + (14 + 29 - bubble_h) as f32 * gs,
                12.0 * gs,
                bubble_h as f32 * gs,
                5,
                185.0,
                (29 - bubble_h) as f32,
                12.0,
                bubble_h as f32,
            );
        }
    }

    fn draw_enchanting_indicators(
        &self,
        context: &mut HudContext<'_>,
        grid_x: f32,
        y: f32,
        font_sz: f32,
        gs: f32,
    ) {
        for row in 0..3 {
            let level = window_property(&self.inventory_window_properties, row as i16).max(0);
            draw_atlas_sprite(
                &mut context.gui.generic54_batch,
                grid_x + 60.0 * gs,
                y + (14.0 + row as f32 * 19.0) * gs,
                108.0 * gs,
                19.0 * gs,
                6,
                0.0,
                if level > 0 { 166.0 } else { 185.0 },
                108.0,
                19.0,
            );
            context.gui.widget_batch.register_button(
                crate::ui::button_ids::ENCHANT_OPTION_BASE + row as u32,
                grid_x + 60.0 * gs,
                y + (14.0 + row as f32 * 19.0) * gs,
                108.0 * gs,
                19.0 * gs,
            );
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                grid_x + 142.0 * gs,
                y + (21.0 + row as f32 * 19.0) * gs,
                &if level > 0 {
                    format_text(
                        context.settings.ui_text.get("rustcraft.enchant.level"),
                        &[&level.to_string()],
                    )
                } else {
                    "-".to_string()
                },
                font_sz * 0.55,
                [0.50, 0.55, 0.32, 1.0],
            );
        }
    }

    fn draw_anvil_indicators(
        &self,
        context: &mut HudContext<'_>,
        x: f32,
        y: f32,
        font_sz: f32,
        gs: f32,
    ) {
        let has_input = self
            .inventory_window_slots
            .first()
            .is_some_and(|slot| !slot.is_empty());
        let has_second = self
            .inventory_window_slots
            .get(1)
            .is_some_and(|slot| !slot.is_empty());
        let has_output = self
            .inventory_window_slots
            .get(2)
            .is_some_and(|slot| !slot.is_empty());
        draw_atlas_sprite(
            &mut context.gui.generic54_batch,
            x + 59.0 * gs,
            y + 20.0 * gs,
            110.0 * gs,
            16.0 * gs,
            7,
            0.0,
            if has_input { 166.0 } else { 182.0 },
            110.0,
            16.0,
        );
        if (has_input || has_second) && !has_output {
            draw_atlas_sprite(
                &mut context.gui.generic54_batch,
                x + 99.0 * gs,
                y + 45.0 * gs,
                28.0 * gs,
                21.0 * gs,
                7,
                176.0,
                0.0,
                28.0,
                21.0,
            );
        }
        let cost = window_property(&self.inventory_window_properties, 0);
        if cost > 0 && has_output {
            let cost_string = cost.to_string();
            let cost_label = format_text(
                &context.settings.ui_text.dynamic("container.repair.cost"),
                &[&cost_string],
            );
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                x + 100.0 * gs,
                y + 67.0 * gs,
                &cost_label,
                font_sz * 0.55,
                if cost >= 40 {
                    [1.0, 0.25, 0.25, 1.0]
                } else {
                    [0.50, 0.80, 0.20, 1.0]
                },
            );
        }
    }

    pub(super) fn draw_inventory_slot(
        &mut self,
        context: &mut HudContext<'_>,
        sx: f32,
        sy: f32,
        slot_size: f32,
        slot: ItemStackView,
        font_sz: f32,
        gs: f32,
        protocol_slot: Option<i16>,
    ) {
        let hovered = context.gui.mouse_x >= sx
            && context.gui.mouse_x <= sx + slot_size
            && context.gui.mouse_y >= sy
            && context.gui.mouse_y <= sy + slot_size;
        if let Some(slot_index) = protocol_slot {
            context.gui.widget_batch.register_button(
                crate::ui::button_ids::INVENTORY_SLOT_BASE + slot_index as u32,
                sx,
                sy,
                slot_size,
                slot_size,
            );
        }
        let count = slot.count;
        if count == 0 {
            return;
        }
        // Track hovered slot for tooltip (avoids one-frame delay from gui_hit_test)
        if hovered && self.pending_tooltip.is_empty() {
            let stack = self.inventory_stack_for_protocol_slot(protocol_slot.unwrap_or(0));
            self.pending_tooltip =
                crate::render::hud::overlays::tooltip_lines_for_stack(context, &stack);
        }
        draw_item_stack(context.gui, sx, sy, slot_size, &slot, gs, self.time);
        if count > 1 {
            self.draw_item_count(context, sx, sy, slot_size, count, font_sz);
        }
    }

    fn draw_item_count(
        &self,
        context: &mut HudContext<'_>,
        sx: f32,
        sy: f32,
        slot_size: f32,
        count: u8,
        font_sz: f32,
    ) {
        let text = count.to_string();
        let text_size = font_sz * (8.0 / 9.0);
        let text_width = text
            .chars()
            .map(|ch| context.gui.font.get_or_render(ch, text_size).advance)
            .sum::<f32>();
        let px = slot_size / 16.0;
        // RenderItem.renderItemOverlayIntoGUI: x + 17 - text_width, y + 9.
        let x = sx + 17.0 * px - text_width;
        let y = sy + 9.0 * px;
        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            x,
            y,
            &text,
            text_size,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    pub(super) fn draw_cursor_stack(
        &mut self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
        slot_size: f32,
        font_sz: f32,
        gs: f32,
    ) {
        let slot = self.inventory_cursor_slot.clone();
        if slot.count == 0 {
            return;
        }
        let x = metrics.mouse_pos[0] - slot_size * 0.45;
        let y = metrics.mouse_pos[1] - slot_size * 0.45;
        context.gui.font_batch.fill_rect(
            x - 2.0 * gs,
            y - 2.0 * gs,
            slot_size + 4.0 * gs,
            slot_size + 4.0 * gs,
            [0.0, 0.0, 0.0, 0.35],
        );
        draw_item_stack(context.gui, x, y, slot_size, &slot, gs, self.time);
        if slot.count > 1 {
            self.draw_item_count(context, x, y, slot_size, slot.count, font_sz);
        }
    }

    pub(super) fn finish_tooltip(
        &mut self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
        font_sz: f32,
        gs: f32,
    ) {
        if self.inventory_cursor_slot.is_empty() {
            crate::render::hud::overlays::draw_item_tooltip(
                context,
                metrics,
                &self.pending_tooltip,
                font_sz,
                gs,
            );
        }
    }

    pub(super) fn creative_visible_entries(
        &self,
        context: &HudContext<'_>,
    ) -> Vec<CreativeItemEntry> {
        let mut entries = creative_tab_entries(context.gui.creative_tab);
        if context.gui.creative_tab == CREATIVE_TAB_SEARCH {
            let query = context.gui.creative_search.trim().to_lowercase();
            if !query.is_empty() {
                entries.retain(|entry| {
                    entry.item_id.to_string().contains(&query)
                        || crate::render::item_icons::item_icon_path(entry.item_id, entry.damage)
                            .is_some_and(|path| path.to_lowercase().contains(&query))
                });
            }
        }
        entries
    }

    pub(super) fn creative_stack_for_slot(
        &self,
        context: &HudContext<'_>,
        slot: usize,
    ) -> ItemStackView {
        if context.gui.creative_tab == CREATIVE_TAB_INVENTORY {
            return ItemStackView::default();
        }
        let items = creative_tab_entries(context.gui.creative_tab);
        let cols = 9usize;
        let rows = 5usize;
        let max_scroll_rows = (items.len() / cols).saturating_sub(rows);
        let scroll_start =
            (context.gui.creative_scroll * max_scroll_rows as f32).round() as usize * cols;
        let Some(item) = items.get(scroll_start + slot).copied() else {
            return ItemStackView::default();
        };
        ItemStackView {
            item_id: item.item_id,
            count: 1,
            damage: item.damage,
            nbt: None,
        }
    }

    fn inventory_stack_for_protocol_slot(&self, protocol_slot: i16) -> ItemStackView {
        if self.inventory_window_id != 0
            && protocol_slot >= 0
            && (protocol_slot as usize) < self.inventory_window_slot_count
        {
            return self
                .inventory_window_slots
                .get(protocol_slot as usize)
                .cloned()
                .unwrap_or_default();
        }

        if self.inventory_window_id != 0 && protocol_slot >= self.inventory_window_slot_count as i16
        {
            let player_slot = protocol_slot - self.inventory_window_slot_count as i16;
            return match player_slot {
                0..=26 => self.inventory_slots[(player_slot + 9) as usize].clone(),
                27..=35 => self.inventory_slots[(player_slot - 27) as usize].clone(),
                _ => ItemStackView::default(),
            };
        }

        match protocol_slot {
            0..=4 => self.inventory_crafting_slots[protocol_slot as usize].clone(),
            5..=8 => self.inventory_armor_slots[(protocol_slot - 5) as usize].clone(),
            9..=35 => self.inventory_slots[protocol_slot as usize].clone(),
            36..=44 => self.inventory_slots[(protocol_slot - 36) as usize].clone(),
            _ => ItemStackView::default(),
        }
    }
}
