use crate::client::inventory::ItemStackView;
use crate::render::gui::widgets::MenuMetrics;
use crate::render::gui_renderer::{
    creative_max_scroll_rows, creative_scrollbar_geometry, creative_tab_entries, CreativeItemEntry,
    GuiRenderer, CREATIVE_TAB_INVENTORY, CREATIVE_TAB_SEARCH,
};
use crate::ui::format::format_text;

use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
const CREATIVE_PANEL_WIDTH: f32 = 195.0;
const CREATIVE_PANEL_HEIGHT: f32 = 136.0;

/// Convert a vanilla sRGB color byte (0-255) to the linear value the GUI
/// pipeline expects. The swapchain is `*_SRGB`, so fragment colors are
/// re-encoded on write; feeding byte/255 directly would brighten the color
/// (e.g. the durability track's dark green 64 would display as ~137).
fn srgb_byte_to_linear(byte: f32) -> f32 {
    let c = (byte / 255.0).clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Vanilla `RenderItem.renderItemOverlayIntoGUI` durability bar: a 13×2 black
/// backdrop at (x+2, y+13) in 16px slot space, a dark 12×1 track, and a
/// green→red foreground whose width and hue scale with remaining durability.
pub(crate) fn draw_durability_bar(
    font_gui: &mut crate::render::gui::GuiVertexBuilder,
    sx: f32,
    sy: f32,
    slot_size: f32,
    item_id: u16,
    damage: u16,
) {
    let max = crate::client::inventory::max_damage(item_id);
    // ItemStack.isItemDamaged: damageable and damage > 0.
    if max == 0 || damage == 0 {
        return;
    }
    let px = slot_size / 16.0;
    let damage = f64::from(damage.min(max));
    let max = f64::from(max);
    let width = (13.0 - damage * 13.0 / max).round() as f32;
    let green = (255.0 - damage * 255.0 / max).round() as f32;
    let red = 255.0 - green;
    let bar_x = sx + 2.0 * px;
    let bar_y = sy + 13.0 * px;
    font_gui.fill_rect(bar_x, bar_y, 13.0 * px, 2.0 * px, [0.0, 0.0, 0.0, 1.0]);
    font_gui.fill_rect(
        bar_x,
        bar_y,
        12.0 * px,
        1.0 * px,
        [
            srgb_byte_to_linear(((red as i32) / 4) as f32),
            srgb_byte_to_linear(64.0),
            0.0,
            1.0,
        ],
    );
    font_gui.fill_rect(
        bar_x,
        bar_y,
        width * px,
        1.0 * px,
        [
            srgb_byte_to_linear(red),
            srgb_byte_to_linear(green),
            0.0,
            1.0,
        ],
    );
}

pub(crate) fn draw_item_stack(
    gui: &mut GuiRenderer,
    sx: f32,
    sy: f32,
    slot_size: f32,
    stack: &ItemStackView,
    gs: f32,
    time: f32,
) {
    if stack.is_empty() {
        return;
    }
    draw_item_icon(gui, sx, sy, slot_size, stack, gs, time);
    draw_durability_bar(
        &mut gui.font_batch,
        sx,
        sy,
        slot_size,
        stack.item_id,
        stack.damage,
    );
    if stack.count > 1 {
        let text = stack.count.to_string();
        let text_size = 9.0 * gs * (8.0 / 9.0);
        let text_width = text
            .chars()
            .map(|ch| gui.font.get_or_render(ch, text_size).advance)
            .sum::<f32>();
        let px = slot_size / 16.0;
        // RenderItem.renderItemOverlayIntoGUI: x + 17 - text_width, y + 9.
        let x = sx + 17.0 * px - text_width;
        let y = sy + 9.0 * px;
        gui.font_batch
            .draw_text(&mut gui.font, x, y, &text, text_size, [1.0, 1.0, 1.0, 1.0]);
    }
}

fn draw_item_icon(
    gui: &mut GuiRenderer,
    sx: f32,
    sy: f32,
    slot_size: f32,
    slot: &ItemStackView,
    gs: f32,
    time: f32,
) {
    let glint = crate::client::inventory::has_glint(slot);
    if crate::render::item_icons::item_icon_path(slot.item_id, slot.damage).is_some()
        && matches!(slot.item_id, 50 | 75 | 76 | 166)
    {
        draw_item_icon_2d_or_block(
            gui,
            sx,
            sy,
            slot_size,
            slot.item_id,
            slot.damage,
            gs,
            time,
            glint,
        );
        return;
    }
    if slot.item_id <= 255 && slot.item_id != 0 {
        let block =
            crate::world::block::Block::from_state((slot.item_id << 4) | (slot.damage & 0x0f));
        if block != crate::world::block::Block::Air
            && block.to_id() == slot.item_id
            && is_full_block_inventory(block)
        {
            if crate::client::block_icon::draw_block_icon(
                &mut gui.block_batch,
                sx + slot_size / 2.0,
                sy + slot_size * 0.5,
                slot_size * 0.55,
                slot.item_id,
                slot.damage,
            ) {
                return;
            }
        }
    }
    draw_item_icon_2d_or_block(
        gui,
        sx,
        sy,
        slot_size,
        slot.item_id,
        slot.damage,
        gs,
        time,
        glint,
    );
}

pub(crate) fn draw_item_icon_2d_or_block(
    gui: &mut GuiRenderer,
    sx: f32,
    sy: f32,
    slot_size: f32,
    item_id: u16,
    damage: u16,
    gs: f32,
    time: f32,
    glint: bool,
) {
    // Bow pulling frames are keyed by damage 1-3 for the first-person
    // hand pass; GUI slots always show the standby icon regardless of
    // durability wear.
    let damage = if item_id == 261 { 0 } else { damage };
    let force_item_icon = matches!(item_id, 50 | 75 | 76 | 166);
    if item_id <= 255 && item_id != 0 && !force_item_icon {
        let block = crate::world::block::Block::from_state((item_id << 4) | (damage & 0x0f));
        if block != crate::world::block::Block::Air
            && block.to_id() == item_id
            && is_full_block_inventory(block)
        {
            if crate::client::block_icon::draw_block_icon(
                &mut gui.block_batch,
                sx + slot_size / 2.0,
                sy + slot_size * 0.5,
                slot_size * 0.55,
                item_id,
                damage,
            ) {
                return;
            }
        }
    }

    let layers = crate::render::item_icons::item_icon_layers(item_id, damage);
    if !layers.is_empty() {
        let margin = 0.0;
        for layer in &layers {
            let Some(index) = crate::render::item_icons::item_icon_entry_index(layer.path) else {
                continue;
            };
            let rect = crate::render::item_icons::item_icon_uv_rect(index);
            gui.item_batch.add_quad(
                sx + margin,
                sy + margin,
                slot_size - 2.0 * margin,
                slot_size - 2.0 * margin,
                rect[0],
                rect[1],
                rect[2] - rect[0],
                rect[3] - rect[1],
                layer.color,
            );
        }
        if glint {
            let tint = crate::client::inventory::glint_tint(time);
            let rect = crate::render::item_icons::item_icon_uv_rect(
                crate::render::item_icons::item_icon_entry_index(layers[0].path).unwrap_or(0),
            );
            gui.item_batch.add_quad(
                sx + margin,
                sy + margin,
                slot_size - 2.0 * margin,
                slot_size - 2.0 * margin,
                rect[0],
                rect[1],
                rect[2] - rect[0],
                rect[3] - rect[1],
                tint,
            );
        }
        return;
    }

    gui.font_batch.fill_rect(
        sx + 2.0 * gs,
        sy + 2.0 * gs,
        slot_size - 4.0 * gs,
        slot_size - 4.0 * gs,
        [0.18, 0.18, 0.22, 0.95],
    );
    gui.font_batch.fill_rect(
        sx + 4.0 * gs,
        sy + 4.0 * gs,
        slot_size - 8.0 * gs,
        3.0 * gs,
        [0.62, 0.62, 0.72, 0.9],
    );
    gui.font_batch.draw_text_centered(
        &mut gui.font,
        sx + slot_size * 0.5,
        sy + slot_size * 0.46,
        "?",
        slot_size * 0.42,
        [0.95, 0.95, 1.0, 1.0],
    );
}

fn is_full_block_inventory(block: crate::world::block::Block) -> bool {
    use crate::world::block::Block;
    // Blocks that look wrong as 3D cubes in inventory — render as flat item instead
    !matches!(
        block,
        Block::GlassPane
            | Block::StainedGlassPane
            | Block::IronBars
            | Block::OakFence
            | Block::SpruceFence
            | Block::BirchFence
            | Block::JungleFence
            | Block::DarkOakFence
            | Block::AcaciaFence
            | Block::NetherBrickFence
            | Block::CobblestoneWall
            | Block::Cobweb
            | Block::Ladder
            | Block::Vine
            | Block::SugarCane
            | Block::Cactus
            | Block::Cake
    )
}

pub(crate) fn window_property(properties: &[(i16, i16)], property: i16) -> i16 {
    properties
        .iter()
        .find(|(key, _)| *key == property)
        .map(|(_, value)| *value)
        .unwrap_or(0)
}

pub(crate) fn vanilla_preview_rotation(
    anchor_x: f32,
    anchor_bottom: f32,
    mouse_pos: [f32; 2],
    gui_scale: f32,
) -> (f32, f32) {
    let scale = gui_scale.max(1.0);
    let mouse_x = (anchor_x - mouse_pos[0]) / scale;
    let mouse_y = (anchor_bottom - 30.0 * scale - mouse_pos[1]) / scale;
    (
        (mouse_x / 40.0).atan() * 20.0,
        (mouse_y / 40.0).atan() * 20.0,
    )
}

pub(crate) fn draw_atlas_sprite(
    builder: &mut crate::render::gui::GuiVertexBuilder,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    cell: usize,
    src_x: f32,
    src_y: f32,
    src_w: f32,
    src_h: f32,
) {
    let cell_x = (cell % 4) as f32 * 256.0;
    let cell_y = (cell / 4) as f32 * 256.0;
    builder.add_quad(
        x,
        y,
        w,
        h,
        (cell_x + src_x) / 1024.0,
        (cell_y + src_y) / 768.0,
        src_w / 1024.0,
        src_h / 768.0,
        [1.0, 1.0, 1.0, 1.0],
    );
}
