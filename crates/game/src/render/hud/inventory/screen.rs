use crate::client::inventory::ItemStackView;
use crate::render::gui::widgets::MenuMetrics;
use crate::render::gui_renderer::{
    creative_max_scroll_rows, creative_scrollbar_geometry, creative_tab_entries, CreativeItemEntry,
    GuiRenderer, CREATIVE_TAB_INVENTORY, CREATIVE_TAB_SEARCH,
};
use crate::ui::format::format_text;

use super::{
    draw_item_icon_2d_or_block, draw_item_stack, vanilla_preview_rotation, CREATIVE_PANEL_HEIGHT,
    CREATIVE_PANEL_WIDTH,
};
use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
impl HudRenderer {
    pub(crate) fn draw_inventory(&mut self, context: &mut HudContext<'_>) {
        let metrics = hud_metrics(context);
        let gs = metrics.gs;
        let font_sz = metrics.font_sz;

        // Vanilla GuiContainer draws drawDefaultBackground() behind the panel.
        context
            .gui
            .overlay_batch
            .fill_world_background(metrics.sw, metrics.sh);

        if self.inventory_window_id != 0 {
            self.draw_container_window(context, &metrics);
            return;
        }

        // Creative mode inventory
        if context.world.gamemode == 1 {
            self.draw_creative_inventory(context, &metrics);
            return;
        }

        self.draw_survival_inventory(context, &metrics, font_sz, gs);
    }

    fn draw_survival_inventory(
        &mut self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
        font_sz: f32,
        gs: f32,
    ) {
        let sw = metrics.sw;
        let sh = metrics.sh;
        context
            .gui
            .font_batch
            .fill_rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.68]);
        let inv_w = 176.0 * gs;
        let inv_h = 166.0 * gs;
        let panel_x = (sw - inv_w) * 0.5;
        let panel_y = (sh - inv_h) * 0.5;

        self.draw_potion_effects_inventory(context, metrics);
        context.gui.inventory_batch.add_quad(
            panel_x,
            panel_y,
            inv_w,
            inv_h,
            0.0,
            0.0,
            176.0 / 256.0,
            166.0 / 256.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        let slot_size = 16.0 * gs;
        let slot_step = 18.0 * gs;
        let grid_x = panel_x + 8.0 * gs;
        let grid_y = panel_y + 84.0 * gs;

        let portrait_x = panel_x + 51.0 * gs;
        let portrait_bottom = panel_y + 75.0 * gs;
        let portrait_px = 30.0 / 16.0 * gs;
        let (portrait_yaw, portrait_pitch) =
            vanilla_preview_rotation(portrait_x, portrait_bottom, metrics.mouse_pos, gs);
        crate::render::entity::player_model::draw_player_preview(
            &mut context.gui.font_batch,
            &mut self.player_preview_cache,
            &context.entity.local_skin,
            portrait_x,
            portrait_bottom - 32.0 * portrait_px,
            portrait_px,
            portrait_yaw,
            portrait_pitch,
            context.entity.local_skin_slim,
            context.settings.skin_parts,
            1.0,
        );

        for i in 0..4 {
            let sx = panel_x + 8.0 * gs;
            let sy = panel_y + (8.0 + i as f32 * 18.0) * gs;
            self.draw_inventory_slot(
                context,
                sx,
                sy,
                slot_size,
                self.inventory_armor_slots[i].clone(),
                font_sz,
                gs,
                Some(5 + i as i16),
            );
        }

        let craft_x = panel_x + 98.0 * gs;
        let craft_y = panel_y + 18.0 * gs;
        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            panel_x + 86.0 * gs,
            panel_y + 16.0 * gs,
            context.settings.ui_text.get("container.crafting"),
            font_sz * 0.64,
            [0.25, 0.25, 0.25, 1.0],
        );
        for row in 0..2 {
            for col in 0..2 {
                let craft_idx = 1 + row * 2 + col;
                let sx = craft_x + col as f32 * slot_step;
                let sy = craft_y + row as f32 * slot_step;
                self.draw_inventory_slot(
                    context,
                    sx,
                    sy,
                    slot_size,
                    self.inventory_crafting_slots[craft_idx].clone(),
                    font_sz,
                    gs,
                    Some(craft_idx as i16),
                );
            }
        }
        let result_x = panel_x + 154.0 * gs;
        let result_y = panel_y + 28.0 * gs;
        self.draw_inventory_slot(
            context,
            result_x,
            result_y,
            slot_size,
            self.inventory_crafting_slots[0].clone(),
            font_sz,
            gs,
            Some(0),
        );

        for row in 0..3 {
            for col in 0..9 {
                let idx = 9 + row * 9 + col;
                let sx = grid_x + col as f32 * slot_step;
                let sy = grid_y + row as f32 * slot_step;
                self.draw_inventory_slot(
                    context,
                    sx,
                    sy,
                    slot_size,
                    self.inventory_slots[idx].clone(),
                    font_sz,
                    gs,
                    Some(idx as i16),
                );
            }
        }

        let hotbar_y = panel_y + 142.0 * gs;
        for col in 0..9 {
            let sx = grid_x + col as f32 * slot_step;
            self.draw_inventory_slot(
                context,
                sx,
                hotbar_y,
                slot_size,
                self.inventory_slots[col].clone(),
                font_sz,
                gs,
                Some(36 + col as i16),
            );
        }

        if self.inventory_slots.iter().all(|slot| slot.count == 0) {
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                sw / 2.0,
                hotbar_y + slot_size + 10.0 * gs,
                context.settings.ui_text.get("rustcraft.inventory.empty"),
                font_sz * 0.72,
                [0.7, 0.7, 0.7, 1.0],
            );
        }

        self.draw_cursor_stack(context, metrics, slot_size, font_sz, gs);
        self.finish_tooltip(context, metrics, font_sz, gs);
    }

    pub(super) fn draw_creative_inventory(
        &mut self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
    ) {
        let gs = metrics.gs;
        let font_sz = metrics.font_sz;
        let slot_size = 16.0 * gs;
        let slot_step = 18.0 * gs;
        let cols = 9usize;
        let rows = 5usize;
        let total_slots = cols * rows;

        // Dark overlay
        context
            .gui
            .font_batch
            .fill_rect(0.0, 0.0, metrics.sw, metrics.sh, [0.0, 0.0, 0.0, 0.68]);

        // Vanilla creative inventory is 195x136 for every tab, including the inventory tab.
        let inv_w = CREATIVE_PANEL_WIDTH * gs;
        let is_inventory_tab = context.gui.creative_tab == CREATIVE_TAB_INVENTORY;
        let inv_h = CREATIVE_PANEL_HEIGHT * gs;
        let panel_x = (metrics.sw - inv_w) * 0.5;
        let panel_y = (metrics.sh - inv_h) * 0.5;

        // Panel background from creative atlas
        // Most tabs (0-10 except search): tab_items.png (cell 1: uv 0.5, 0)
        // Tab 5 (search): tab_item_search.png (cell 3: uv 0.5, 0.5)
        // Tab 11 (inventory): tab_inventory.png (cell 2: uv 0.0, 0.5)
        let (panel_u, panel_v, panel_tex_h) = match context.gui.creative_tab {
            CREATIVE_TAB_SEARCH => (0.5, 0.5, 136.0),
            CREATIVE_TAB_INVENTORY => (0.0, 0.5, 136.0),
            _ => (0.5, 0.0, 136.0),
        };
        context.gui.creative_batch.add_quad(
            panel_x,
            panel_y,
            inv_w,
            inv_h,
            panel_u,
            panel_v,
            195.0 / 512.0,
            panel_tex_h / 512.0,
            [1.0, 1.0, 1.0, 1.0],
        );

        if is_inventory_tab {
            let portrait_x = panel_x + 43.0 * gs;
            let portrait_bottom = panel_y + 45.0 * gs;
            let portrait_px = 20.0 / 16.0 * gs;
            let (portrait_yaw, portrait_pitch) =
                vanilla_preview_rotation(portrait_x, portrait_bottom, metrics.mouse_pos, gs);
            crate::render::entity::player_model::draw_player_preview(
                &mut context.gui.font_batch,
                &mut self.player_preview_cache,
                &context.entity.local_skin,
                portrait_x,
                portrait_bottom - 32.0 * portrait_px,
                portrait_px,
                portrait_yaw,
                portrait_pitch,
                context.entity.local_skin_slim,
                context.settings.skin_parts,
                1.0,
            );
        }

        if context.gui.creative_tab == CREATIVE_TAB_SEARCH {
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                panel_x + 82.0 * gs,
                panel_y + 6.0 * gs,
                &context.gui.creative_search,
                font_sz * 0.64,
                [1.0, 1.0, 1.0, 1.0],
            );
        }

        // Tab icons: (item_id, damage)
        let tab_icons: [(u16, u16); 12] = [
            (45, 0),  // Building Blocks: brick_block
            (175, 2), // Decorations: double_plant (paeonia)
            (331, 0), // Redstone: redstone
            (27, 0),  // Transportation: golden_rail
            (327, 0), // Misc: lava_bucket
            (345, 0), // Search: compass
            (260, 0), // Food: apple
            (258, 0), // Tools: iron_axe
            (283, 0), // Combat: golden_sword
            (373, 0), // Brewing: potionitem
            (280, 0), // Materials: stick
            (54, 0),  // Inventory: chest
        ];

        // --- Tabs: 12 tabs, 6 per row. Tab backgrounds from tabs.png (cell 0) ---
        let tab_w_tex = 28.0f32;
        let tab_h_tex = 32.0f32;
        let tab_w = tab_w_tex * gs;
        let tab_h = tab_h_tex * gs;

        for i in 0..12 {
            let col = i % 6;
            let is_top = i < 6;
            let is_selected = i == context.gui.creative_tab;

            // Screen position (MC 1.8.9 layout)
            let tab_x = if col == 5 {
                panel_x + inv_w - tab_w
            } else {
                panel_x + col as f32 * tab_w
            };
            let tab_y = if is_top {
                panel_y - 28.0 * gs
            } else {
                panel_y + inv_h - 4.0 * gs
            };

            // Register tab as clickable button
            context.gui.widget_batch.register_button(
                crate::ui::button_ids::CREATIVE_TAB_BASE + i as u32,
                tab_x,
                tab_y,
                tab_w,
                tab_h,
            );

            // Texture UV in tabs.png (cell 0 of creative atlas)
            let tex_u = col as f32 * tab_w_tex;
            let tex_v = match (is_top, is_selected) {
                (true, false) => 0.0,   // top unselected
                (true, true) => 32.0,   // top selected
                (false, false) => 64.0, // bottom unselected
                (false, true) => 96.0,  // bottom selected
            };

            // Draw tab background from creative atlas
            context.gui.creative_batch.add_quad(
                tab_x,
                tab_y,
                tab_w,
                tab_h,
                tex_u / 512.0,
                tex_v / 512.0,
                tab_w_tex / 512.0,
                tab_h_tex / 512.0,
                [1.0, 1.0, 1.0, 1.0],
            );

            // Tab icon
            let (icon_id, icon_damage) = tab_icons[i];
            draw_item_icon_2d_or_block(
                context.gui,
                tab_x + (tab_w - slot_size) * 0.5,
                tab_y + 6.0 * gs,
                slot_size,
                icon_id,
                icon_damage,
                gs,
                self.time,
                false,
            );
        }

        // --- Item grid (9 columns x 5 rows = 45 slots) ---
        let grid_x = panel_x + 9.0 * gs;
        let grid_y = panel_y + 18.0 * gs;

        let items = if is_inventory_tab {
            vec![]
        } else {
            self.creative_visible_entries(context)
        };
        let max_scroll_rows = creative_max_scroll_rows(items.len());
        let scroll_start =
            (context.gui.creative_scroll * max_scroll_rows as f32).round() as usize * cols;
        let visible_count = total_slots.min(items.len().saturating_sub(scroll_start));

        // Draw slot backgrounds and items (skip for inventory tab — it draws its own layout)
        if !is_inventory_tab {
            for i in 0..total_slots {
                let col = i % cols;
                let row = i / cols;
                let sx = grid_x + col as f32 * slot_step;
                let sy = grid_y + row as f32 * slot_step;

                // Draw slot background for all positions
                context.gui.widget_batch.fill_rect(
                    sx,
                    sy,
                    slot_size,
                    slot_size,
                    [0.0, 0.0, 0.0, 0.5],
                );

                if i < visible_count {
                    context.gui.widget_batch.register_button(
                        crate::ui::button_ids::CREATIVE_SLOT_BASE + i as u32,
                        sx,
                        sy,
                        slot_size,
                        slot_size,
                    );
                    let hovered = metrics.mouse_pos[0] >= sx
                        && metrics.mouse_pos[0] <= sx + slot_size
                        && metrics.mouse_pos[1] >= sy
                        && metrics.mouse_pos[1] <= sy + slot_size;
                    if hovered {
                        context.gui.font_batch.fill_rect(
                            sx,
                            sy,
                            slot_size,
                            slot_size,
                            [1.0, 1.0, 1.0, 0.34],
                        );
                        if self.pending_tooltip.is_empty() {
                            let stack = self.creative_stack_for_slot(context, i);
                            self.pending_tooltip =
                                crate::render::hud::overlays::tooltip_lines_for_stack(
                                    context, &stack,
                                );
                        }
                    }
                    let item = items[scroll_start + i];
                    let stack = crate::client::inventory::ItemStackView {
                        item_id: item.item_id,
                        count: 1,
                        damage: item.damage,
                        nbt: None,
                    };
                    draw_item_stack(context.gui, sx, sy, slot_size, &stack, gs, self.time);
                }
            }

            // Vanilla always renders the fixed-size thumb; U=244 is its disabled state.
            let scrollbar = creative_scrollbar_geometry(metrics.sw, metrics.sh, gs);
            let can_scroll = max_scroll_rows > 0;
            let thumb_y = scrollbar.y
                + context.gui.creative_scroll.clamp(0.0, 1.0)
                    * (scrollbar.height - 17.0 * scrollbar.scale);
            context.gui.creative_batch.add_quad(
                scrollbar.x,
                thumb_y,
                12.0 * gs,
                scrollbar.thumb_height,
                if can_scroll {
                    232.0 / 512.0
                } else {
                    244.0 / 512.0
                },
                0.0,
                12.0 / 512.0,
                15.0 / 512.0,
                [1.0, 1.0, 1.0, 1.0],
            );
        }

        // --- Player inventory section ---
        if is_inventory_tab {
            // Vanilla creative inventory tab layout:
            // armor slots are 2x2 on the left, inventory on the right, hotbar at the bottom.
            for j in 5..9 {
                let rel = j - 5;
                let col = rel / 2;
                let row = rel % 2;
                let sx = panel_x + (9.0 + col as f32 * 54.0) * gs;
                let sy = panel_y + (6.0 + row as f32 * 27.0) * gs;
                self.draw_inventory_slot(
                    context,
                    sx,
                    sy,
                    slot_size,
                    self.inventory_armor_slots[(j - 5) as usize].clone(),
                    font_sz,
                    gs,
                    Some(j as i16),
                );
            }

            let inv_grid_y = panel_y + 54.0 * gs;
            for row in 0..3 {
                for col in 0..9 {
                    let idx = 9 + row * 9 + col;
                    let sx = panel_x + (9.0 + col as f32 * 18.0) * gs;
                    let sy = inv_grid_y + row as f32 * slot_step;
                    self.draw_inventory_slot(
                        context,
                        sx,
                        sy,
                        slot_size,
                        self.inventory_slots[idx].clone(),
                        font_sz,
                        gs,
                        Some(idx as i16),
                    );
                }
            }

            let hotbar_y = panel_y + 112.0 * gs;
            for i in 0..9 {
                let sx = panel_x + (9.0 + i as f32 * 18.0) * gs;
                self.draw_inventory_slot(
                    context,
                    sx,
                    hotbar_y,
                    slot_size,
                    self.inventory_slots[i].clone(),
                    font_sz,
                    gs,
                    Some((36 + i) as i16),
                );
            }

            let trash_x = panel_x + 173.0 * gs;
            let trash_y = panel_y + 112.0 * gs;
            context.gui.widget_batch.register_button(
                crate::ui::button_ids::CREATIVE_TRASH,
                trash_x,
                trash_y,
                slot_size,
                slot_size,
            );
            let trash_hovered = metrics.mouse_pos[0] >= trash_x
                && metrics.mouse_pos[0] <= trash_x + slot_size
                && metrics.mouse_pos[1] >= trash_y
                && metrics.mouse_pos[1] <= trash_y + slot_size;
            if trash_hovered {
                context.gui.font_batch.fill_rect(
                    trash_x,
                    trash_y,
                    slot_size,
                    slot_size,
                    [1.0, 1.0, 1.0, 0.34],
                );
                if self.pending_tooltip.is_empty() {
                    self.pending_tooltip = vec![TooltipLine {
                        text: context.settings.ui_text.dynamic("inventory.binSlot"),
                        color: [1.0, 1.0, 1.0, 1.0],
                    }];
                }
            }
        } else {
            // Non-inventory tabs: only show hotbar
            let hotbar_y = panel_y + 112.0 * gs;
            for i in 0..9 {
                let sx = grid_x + i as f32 * slot_step;
                let slot = self.inventory_slots[i].clone();
                self.draw_inventory_slot(
                    context,
                    sx,
                    hotbar_y,
                    slot_size,
                    slot,
                    font_sz,
                    gs,
                    Some((36 + i) as i16),
                );
            }
        }

        // Cursor stack
        self.draw_cursor_stack(context, metrics, slot_size, font_sz, gs);
        self.finish_tooltip(context, metrics, font_sz, gs);
    }
}
