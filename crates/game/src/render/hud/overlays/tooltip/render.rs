use crate::client::inventory::ItemStackView;
use crate::net::nbt::NbtTag;
use crate::render::gui::widgets::{draw_button, MenuMetrics};
use crate::render::hooks::ScriptDrawCommand;
use crate::ui::format::format_text;
use std::collections::HashMap;

use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
pub(super) const HIDE_ENCHANTMENTS: i32 = 0x01;
pub(super) const HIDE_ATTRIBUTES: i32 = 0x02;
pub(super) const HIDE_UNBREAKABLE: i32 = 0x04;
pub(super) const HIDE_CAN_DESTROY: i32 = 0x08;
pub(super) const HIDE_CAN_PLACE: i32 = 0x10;
pub(super) const HIDE_ADDITIONAL: i32 = 0x20;
pub(super) const TOOLTIP_BLUE: [f32; 4] = [0.56, 0.56, 1.0, 1.0];
pub(super) const TOOLTIP_GRAY: [f32; 4] = [0.62, 0.62, 0.62, 1.0];
pub(super) const TOOLTIP_DARK_GRAY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];

impl HudRenderer {
    pub(crate) fn draw_tooltip(&self, context: &mut HudContext<'_>) {
        let metrics = hud_metrics(context);
        if !self.pending_tooltip.is_empty() {
            draw_item_tooltip(
                context,
                &metrics,
                &self.pending_tooltip,
                metrics.font_sz,
                metrics.gs,
            );
            return;
        }
        if self.hovered_tooltip.is_empty() {
            return;
        }
        let lines = self
            .hovered_tooltip
            .iter()
            .enumerate()
            .map(|(index, text)| TooltipLine {
                text: text.clone(),
                color: if index == 0 {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    TOOLTIP_GRAY
                },
            })
            .collect::<Vec<_>>();
        draw_item_tooltip(context, &metrics, &lines, metrics.font_sz, metrics.gs);
    }
}

/// Build the full vanilla tooltip for an item stack: custom name, enchantments,
/// potion metadata, lore, block lists, attribute modifiers and advanced debug
/// lines. Ported from the old `Renderer::tooltip_lines_for_stack`.
pub(crate) fn tooltip_lines_for_stack(
    context: &HudContext<'_>,
    stack: &ItemStackView,
) -> Vec<TooltipLine> {
    if stack.is_empty() {
        return Vec::new();
    }

    let root = stack
        .nbt
        .as_deref()
        .and_then(|bytes| crate::net::nbt::parse_root(bytes).ok());
    let compound = root.as_ref().and_then(NbtTag::as_compound);
    let display = compound
        .and_then(|root| root.get("display"))
        .and_then(NbtTag::as_compound);
    let hide_flags = compound
        .and_then(|root| root.get("HideFlags"))
        .and_then(NbtTag::as_i32)
        .unwrap_or(0);

    let custom_name = display
        .and_then(|display| display.get("Name"))
        .and_then(NbtTag::as_str)
        .filter(|name| !name.is_empty());
    let mut lines = vec![TooltipLine {
        text: custom_name
            .map(strip_mc_formatting)
            .unwrap_or_else(|| item_display_name(context, stack.item_id, stack.damage)),
        color: if custom_name.is_some() {
            [0.68, 0.33, 1.0, 1.0]
        } else {
            [1.0, 1.0, 1.0, 1.0]
        },
    }];

    if hide_flags & HIDE_ENCHANTMENTS == 0 {
        for enchantment in enchantments_from(compound, "ench")
            .into_iter()
            .chain(enchantments_from(compound, "StoredEnchantments"))
        {
            lines.push(TooltipLine {
                text: enchantment_display_name(context, enchantment.id, enchantment.level),
                color: TOOLTIP_BLUE,
            });
        }
    }

    if hide_flags & HIDE_ADDITIONAL == 0 {
        append_extra_stack_details(context, stack, compound, &mut lines);
    }

    if hide_flags & HIDE_UNBREAKABLE == 0
        && compound
            .and_then(|root| root.get("Unbreakable"))
            .and_then(NbtTag::as_i32)
            .unwrap_or(0)
            != 0
    {
        lines.push(TooltipLine {
            text: context.settings.ui_text.dynamic("item.unbreakable"),
            color: TOOLTIP_BLUE,
        });
    }

    if let Some(lore) = display
        .and_then(|display| display.get("Lore"))
        .and_then(NbtTag::as_list)
    {
        for line in lore.iter().filter_map(NbtTag::as_str) {
            lines.push(TooltipLine {
                text: strip_mc_formatting(line),
                color: [0.68, 0.33, 1.0, 1.0],
            });
        }
    }

    if hide_flags & HIDE_CAN_DESTROY == 0 {
        append_block_list(context, compound, "CanDestroy", "item.canBreak", &mut lines);
    }
    if hide_flags & HIDE_CAN_PLACE == 0 {
        append_block_list(context, compound, "CanPlaceOn", "item.canPlace", &mut lines);
    }
    if hide_flags & HIDE_ATTRIBUTES == 0 {
        append_attribute_modifiers(context, compound, &mut lines);
    }

    if context.settings.advanced_tooltips {
        lines.push(TooltipLine {
            text: format!("#{:04} / {}", stack.item_id, stack.damage),
            color: TOOLTIP_DARK_GRAY,
        });
        if let Some(nbt) = &stack.nbt {
            let tag_count = compound.map(|root| root.len()).unwrap_or(0);
            lines.push(TooltipLine {
                text: format_text(
                    &context.settings.ui_text.dynamic("rustcraft.tooltip.nbt"),
                    &[&tag_count.to_string(), &nbt.len().to_string()],
                ),
                color: TOOLTIP_DARK_GRAY,
            });
        }
    }

    lines
}

/// Draw a vanilla-style tooltip frame (dark backdrop, purple outline) and its
/// text lines. Ported from the old `Renderer::draw_item_tooltip`.
pub(crate) fn draw_item_tooltip(
    context: &mut HudContext<'_>,
    metrics: &MenuMetrics,
    lines: &[TooltipLine],
    font_sz: f32,
    gs: f32,
) {
    if lines.is_empty() {
        return;
    }

    let sw = metrics.sw;
    let sh = metrics.sh;
    let text_size = font_sz * 0.62;
    let line_h = 10.0 * gs;
    let content_w = lines
        .iter()
        .map(|line| {
            context
                .gui
                .font
                .text_width(&strip_mc_formatting(&line.text), text_size)
        })
        .fold(0.0_f32, f32::max);
    let w = content_w + 8.0 * gs;
    // Vanilla leaves a two-pixel gap between the item name and its details.
    let detail_gap = (lines.len() > 1) as u8 as f32 * 2.0 * gs;
    let h = 8.0 * gs + lines.len() as f32 * line_h + detail_gap;
    let mut x = metrics.mouse_pos[0] + 12.0 * gs;
    let mut y = metrics.mouse_pos[1] - 12.0 * gs;
    if x + w > sw - 4.0 * gs {
        x = metrics.mouse_pos[0] - w - 12.0 * gs;
    }
    if y + h > sh - 4.0 * gs {
        y = sh - h - 4.0 * gs;
    }
    x = x.max(4.0 * gs);
    y = y.max(4.0 * gs);

    context.gui.font_batch.fill_rect(
        x - 3.0 * gs,
        y - 4.0 * gs,
        w + 6.0 * gs,
        h + 6.0 * gs,
        [0.05, 0.00, 0.08, 0.94],
    );
    context.gui.font_batch.fill_rect(
        x - 2.0 * gs,
        y - 3.0 * gs,
        w + 4.0 * gs,
        1.0 * gs,
        [0.22, 0.06, 0.34, 1.0],
    );
    context.gui.font_batch.fill_rect(
        x - 2.0 * gs,
        y + h + 2.0 * gs,
        w + 4.0 * gs,
        1.0 * gs,
        [0.22, 0.06, 0.34, 1.0],
    );
    context.gui.font_batch.fill_rect(
        x - 2.0 * gs,
        y - 3.0 * gs,
        1.0 * gs,
        h + 6.0 * gs,
        [0.22, 0.06, 0.34, 1.0],
    );
    context.gui.font_batch.fill_rect(
        x + w + 1.0 * gs,
        y - 3.0 * gs,
        1.0 * gs,
        h + 6.0 * gs,
        [0.22, 0.06, 0.34, 1.0],
    );

    for (idx, line) in lines.iter().enumerate() {
        let text = strip_mc_formatting(&line.text);
        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            x + 3.0 * gs,
            y + 3.0 * gs + idx as f32 * line_h + (idx > 0) as u8 as f32 * 2.0 * gs,
            &text,
            text_size,
            line.color,
        );
    }
}
use super::text::*;
use super::translations::*;
