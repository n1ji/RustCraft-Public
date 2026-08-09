use crate::client::inventory::ItemStackView;
use crate::net::nbt::NbtTag;
use crate::render::gui::widgets::{draw_button, MenuMetrics};
use crate::render::hooks::ScriptDrawCommand;
use crate::ui::format::format_text;
use std::collections::HashMap;

use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
impl HudRenderer {
    pub(crate) fn draw_book_editor(&self, context: &mut HudContext<'_>) {
        if !self.book_editor_open {
            return;
        }
        let metrics = hud_metrics(context);
        let sw = metrics.sw;
        let sh = metrics.sh;
        let gs = metrics.gs;
        let font_sz = metrics.font_sz * 0.68;
        let book_w = 192.0 * gs;
        let book_h = 192.0 * gs;
        let x = (sw - book_w) * 0.5;
        let y = (sh - book_h) * 0.5;
        let page_x = x + 14.0 * gs;
        let page_y = y + 29.0 * gs;
        let page_w = book_w - 28.0 * gs;
        let page_h = 112.0 * gs;

        context
            .gui
            .font_batch
            .fill_rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.62]);
        context
            .gui
            .font_batch
            .fill_rect(x, y, book_w, book_h, [0.37, 0.23, 0.12, 1.0]);
        context.gui.font_batch.fill_rect(
            x + 3.0 * gs,
            y + 3.0 * gs,
            book_w - 6.0 * gs,
            book_h - 6.0 * gs,
            [0.92, 0.83, 0.62, 1.0],
        );
        context.gui.font_batch.fill_rect(
            x + book_w * 0.5 - gs,
            y + 4.0 * gs,
            2.0 * gs,
            book_h - 8.0 * gs,
            [0.48, 0.31, 0.16, 0.42],
        );

        if self.book_signing {
            self.draw_book_signing(context, &metrics, x, y, book_w, book_h, font_sz);
        } else {
            self.draw_book_page(
                context, &metrics, x, y, book_w, book_h, page_x, page_y, page_w, page_h, font_sz,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_book_page(
        &self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
        x: f32,
        y: f32,
        book_w: f32,
        book_h: f32,
        page_x: f32,
        page_y: f32,
        page_w: f32,
        page_h: f32,
        font_sz: f32,
    ) {
        let gs = metrics.gs;
        let page = self
            .book_pages
            .get(self.book_page)
            .map(String::as_str)
            .unwrap_or("");
        let hovered = metrics.mouse_pos[0] >= page_x
            && metrics.mouse_pos[0] <= page_x + page_w
            && metrics.mouse_pos[1] >= page_y
            && metrics.mouse_pos[1] <= page_y + page_h;
        context.gui.widget_batch.register_button(
            crate::ui::button_ids::BOOK_PAGE_FIELD,
            page_x,
            page_y,
            page_w,
            page_h,
        );
        if hovered {
            context
                .gui
                .font_batch
                .fill_rect(page_x, page_y, page_w, page_h, [1.0, 1.0, 1.0, 0.14]);
        }
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            x + book_w * 0.5,
            y + 10.0 * gs,
            "Book & Quill",
            font_sz,
            [0.20, 0.12, 0.06, 1.0],
        );
        for (line, text) in book_lines(page, 24).into_iter().take(10).enumerate() {
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                page_x + 3.0 * gs,
                page_y + line as f32 * 11.0 * gs,
                &text,
                font_sz,
                [0.15, 0.09, 0.04, 1.0],
            );
        }
        let cursor_y = page_y + book_lines(page, 24).len().min(9) as f32 * 11.0 * gs;
        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            page_x + 3.0 * gs,
            cursor_y,
            "_",
            font_sz,
            [0.15, 0.09, 0.04, 1.0],
        );
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            x + book_w * 0.5,
            y + 145.0 * gs,
            &format!("Page {} of {}", self.book_page + 1, self.book_pages.len()),
            font_sz * 0.9,
            [0.25, 0.15, 0.08, 1.0],
        );

        let button_y = y + book_h - 25.0 * gs;
        if self.book_page > 0 {
            draw_button(
                metrics,
                &mut context.gui.widget_batch,
                &mut context.gui.font_batch,
                crate::ui::button_ids::BOOK_PREVIOUS_PAGE,
                [x + 8.0 * gs, button_y, 34.0 * gs, 18.0 * gs],
                "<",
                &mut context.gui.font,
            );
        }
        draw_button(
            metrics,
            &mut context.gui.widget_batch,
            &mut context.gui.font_batch,
            crate::ui::button_ids::BOOK_NEXT_PAGE,
            [x + book_w - 42.0 * gs, button_y, 34.0 * gs, 18.0 * gs],
            ">",
            &mut context.gui.font,
        );
        draw_button(
            metrics,
            &mut context.gui.widget_batch,
            &mut context.gui.font_batch,
            crate::ui::button_ids::BOOK_DONE,
            [x + 52.0 * gs, button_y, 42.0 * gs, 18.0 * gs],
            "Done",
            &mut context.gui.font,
        );
        draw_button(
            metrics,
            &mut context.gui.widget_batch,
            &mut context.gui.font_batch,
            crate::ui::button_ids::BOOK_SIGN,
            [x + book_w - 94.0 * gs, button_y, 42.0 * gs, 18.0 * gs],
            "Sign",
            &mut context.gui.font,
        );
    }

    fn draw_book_signing(
        &self,
        context: &mut HudContext<'_>,
        metrics: &MenuMetrics,
        x: f32,
        y: f32,
        book_w: f32,
        book_h: f32,
        font_sz: f32,
    ) {
        let gs = metrics.gs;
        let title_x = x + 28.0 * gs;
        let title_y = y + 66.0 * gs;
        let title_w = book_w - 56.0 * gs;
        let title_hovered = metrics.mouse_pos[0] >= title_x
            && metrics.mouse_pos[0] <= title_x + title_w
            && metrics.mouse_pos[1] >= title_y
            && metrics.mouse_pos[1] <= title_y + 18.0 * gs;
        context.gui.widget_batch.register_button(
            crate::ui::button_ids::BOOK_TITLE_FIELD,
            title_x,
            title_y,
            title_w,
            18.0 * gs,
        );
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            x + book_w * 0.5,
            y + 36.0 * gs,
            "Enter book title",
            font_sz * 1.2,
            [0.18, 0.10, 0.05, 1.0],
        );
        context.gui.font_batch.fill_rect(
            title_x,
            title_y,
            title_w,
            18.0 * gs,
            [0.20, 0.12, 0.06, 0.25],
        );
        if title_hovered {
            context.gui.font_batch.fill_rect(
                title_x,
                title_y,
                title_w,
                2.0 * gs,
                [0.50, 0.30, 0.12, 0.9],
            );
        }
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            x + book_w * 0.5,
            title_y + 4.0 * gs,
            &format!("{}_", self.book_title),
            font_sz,
            [0.14, 0.08, 0.03, 1.0],
        );
        let button_y = y + book_h - 30.0 * gs;
        draw_button(
            metrics,
            &mut context.gui.widget_batch,
            &mut context.gui.font_batch,
            crate::ui::button_ids::BOOK_CANCEL_SIGN,
            [x + 32.0 * gs, button_y, 56.0 * gs, 20.0 * gs],
            "Cancel",
            &mut context.gui.font,
        );
        if !self.book_title.trim().is_empty() {
            draw_button(
                metrics,
                &mut context.gui.widget_batch,
                &mut context.gui.font_batch,
                crate::ui::button_ids::BOOK_FINALIZE,
                [x + book_w - 88.0 * gs, button_y, 56.0 * gs, 20.0 * gs],
                "Finalize",
                &mut context.gui.font,
            );
        }
    }
}

fn book_lines(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for ch in text.chars() {
        if ch == '\n' {
            lines.push(String::new());
        } else if let Some(line) = lines.last_mut() {
            line.push(ch);
            if line.chars().count() >= width {
                lines.push(String::new());
            }
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// Item tooltips
// ---------------------------------------------------------------------------
