use super::status::hud_metrics;
use super::{HudContext, HudRenderer};

impl HudRenderer {
    pub(super) fn draw_chat(&mut self, context: &mut HudContext<'_>) {
        if !self.chat_overlay && !self.chat_open {
            return;
        }
        let metrics = hud_metrics(context);
        let gs = metrics.gs;
        let row_height = 11.0 * gs;
        let max_visible = if self.chat_open {
            self.chat_height.clamp(1, 30) as usize
        } else {
            6
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.chat_visible_time = now;
        self.chat_alpha = if self.chat_open {
            1.0
        } else {
            (16.0 - (now - self.chat_last_message_time) as f32).clamp(0.0, 1.0)
        };
        if self.chat_alpha <= 0.01 && !self.chat_open {
            return;
        }

        let alpha = self.chat_alpha;
        let chat_x = 4.0 * gs;
        let chat_width =
            (metrics.sw * self.chat_width.clamp(0.1, 1.0)).min(metrics.sw - 8.0 * gs) + 4.0 * gs;
        let avatar_size = (row_height - 2.0 * gs).max(0.0);
        let avatar_width = if self.chat_player_avatars {
            avatar_size + 2.0 * gs
        } else {
            0.0
        };
        let text_x = chat_x + gs + avatar_width;
        let text_width = (chat_width - avatar_width - 4.0 * gs).max(1.0);
        let text_size = metrics.font_sz * 0.72;
        let mut rows = Vec::new();
        for (index, line) in self.chat_lines.iter().enumerate() {
            let face = self.chat_faces.get(index).cloned().flatten();
            for wrapped in wrap_line(&context.gui.font, line, text_size, text_width) {
                rows.push((wrapped, face));
            }
        }

        let max_scroll = rows.len().saturating_sub(max_visible);
        self.chat_scroll = self.chat_scroll.min(max_scroll);
        let start = rows
            .len()
            .saturating_sub(max_visible.saturating_add(self.chat_scroll));
        let end = (start + max_visible).min(rows.len().saturating_sub(self.chat_scroll));
        let visible = start..end;
        let visible_count = visible.len();
        let chat_bottom = metrics.sh - 30.0 * gs;
        if self.chat_background && visible_count > 0 {
            context.gui.font_batch.fill_rect(
                chat_x,
                chat_bottom - visible_count as f32 * row_height - 2.0 * gs,
                chat_width,
                visible_count as f32 * row_height + 2.0 * gs,
                [0.0, 0.0, 0.0, 0.78 * alpha],
            );
        }
        for (visible_index, row_index) in visible.enumerate() {
            let (line, face) = &rows[row_index];
            let y = chat_bottom - (visible_count - visible_index) as f32 * row_height;
            if self.chat_player_avatars {
                if let Some(face) = face {
                    context.gui.font_batch.draw_pixel_face(
                        chat_x + 2.0 * gs,
                        y,
                        avatar_size / 8.0,
                        face,
                    );
                }
            }
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                text_x,
                y,
                line,
                text_size,
                [0.9, 0.9, 0.9, alpha],
            );
        }

        if self.chat_open {
            self.draw_chat_input(context, &metrics, chat_x);
        }
    }

    fn draw_chat_input(
        &self,
        context: &mut HudContext<'_>,
        metrics: &crate::render::gui::widgets::MenuMetrics,
        chat_x: f32,
    ) {
        let gs = metrics.gs;
        let input_y = metrics.sh - 28.0 * gs;
        let input_width = (metrics.sw * 0.5).min(450.0 * gs);
        context.gui.font_batch.fill_rect(
            chat_x,
            input_y,
            input_width,
            18.0 * gs,
            [0.0, 0.0, 0.0, 0.78],
        );
        context.gui.widget_batch.register_button(
            crate::ui::button_ids::CHAT_INPUT,
            chat_x,
            input_y,
            input_width,
            18.0 * gs,
        );
        context.gui.font_batch.draw_text(
            &mut context.gui.font,
            chat_x + 4.0 * gs,
            input_y + 5.0 * gs,
            &format!(">{}", self.chat_input),
            metrics.font_sz,
            [1.0; 4],
        );
        let button_x = chat_x + input_width + 3.0 * gs;
        let button_width = 16.0 * gs;
        let button_height = 8.0 * gs;
        for (id, y, text) in [
            (crate::ui::button_ids::CHAT_SCROLL_UP, input_y, "^"),
            (
                crate::ui::button_ids::CHAT_SCROLL_DOWN,
                input_y + 10.0 * gs,
                "v",
            ),
        ] {
            let hovered = metrics.mouse_pos[0] >= button_x
                && metrics.mouse_pos[0] <= button_x + button_width
                && metrics.mouse_pos[1] >= y
                && metrics.mouse_pos[1] <= y + button_height;
            context
                .gui
                .widget_batch
                .register_button(id, button_x, y, button_width, button_height);
            context.gui.font_batch.fill_rect(
                button_x,
                y,
                button_width,
                button_height,
                if hovered {
                    [0.32, 0.32, 0.32, 0.92]
                } else {
                    [0.0, 0.0, 0.0, 0.78]
                },
            );
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                button_x + button_width * 0.5,
                y - 2.0 * gs,
                text,
                metrics.font_sz * 0.65,
                [0.9, 0.9, 0.9, 1.0],
            );
        }
    }

    pub(super) fn draw_player_list(&self, context: &mut HudContext<'_>) {
        if !self.player_list_open || self.player_list.is_empty() || self.chat_open {
            return;
        }
        let metrics = hud_metrics(context);
        let gs = metrics.gs;
        let font_size = metrics.font_sz * 0.72;
        let avatar_width = if self.tab_player_avatars {
            10.0 * gs
        } else {
            0.0
        };
        let ping_width = context.gui.font.text_width("9999ms", font_size * 0.86);
        let max_name_width = self
            .player_list
            .iter()
            .map(|(name, _, _)| context.gui.font.text_width(name, font_size))
            .fold(40.0 * gs, f32::max);
        let column_width = (max_name_width + avatar_width + ping_width + 24.0 * gs).max(90.0 * gs);
        let columns = ((self.player_list.len() + 19) / 20).clamp(1, 4);
        let rows = ((self.player_list.len() + columns - 1) / columns).min(20);
        let header = split_lines(self.tab_header.as_deref().unwrap_or(""), 3);
        let footer = split_lines(self.tab_footer.as_deref().unwrap_or(""), 3);
        let line_height = 11.0 * gs;
        let panel_width = (columns as f32 * column_width + 12.0 * gs).min(metrics.sw * 0.85);
        let panel_height = (rows + header.len() + footer.len()) as f32 * line_height + 28.0 * gs;
        let x = (metrics.sw - panel_width) * 0.5;
        let y = 18.0 * gs;
        context
            .gui
            .font_batch
            .fill_rect(x, y, panel_width, panel_height, [0.0, 0.0, 0.0, 0.72]);
        context
            .gui
            .font_batch
            .fill_rect(x, y, panel_width, 1.0 * gs, [0.55, 0.55, 0.55, 0.9]);
        let mut cursor_y = y + 5.0 * gs;
        for line in &header {
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                metrics.sw * 0.5,
                cursor_y,
                line,
                font_size,
                [0.92, 0.92, 0.92, 1.0],
            );
            cursor_y += line_height;
        }
        context.gui.font_batch.draw_text_centered(
            &mut context.gui.font,
            metrics.sw * 0.5,
            cursor_y,
            &format!("Players ({})", self.player_list.len()),
            metrics.font_sz,
            [1.0; 4],
        );
        let list_y = cursor_y + 15.0 * gs;
        for (index, (name, ping, gamemode)) in self.player_list.iter().enumerate() {
            let column = index / rows;
            let row = index % rows;
            let cell_x = x + 6.0 * gs + column as f32 * column_width;
            let cell_y = list_y + row as f32 * line_height;
            let mut text_x = cell_x;
            if self.tab_player_avatars {
                if let Some(face) = self.player_list_faces.get(index) {
                    context
                        .gui
                        .font_batch
                        .draw_pixel_face(cell_x, cell_y, gs, face);
                    text_x += avatar_width;
                }
            }
            let mode = match *gamemode {
                1 => "C",
                2 => "A",
                3 => "S",
                _ => "",
            };
            let max_name = (column_width - avatar_width - ping_width - 10.0 * gs).max(8.0 * gs);
            let display = fit_text(
                &context.gui.font,
                &format!("{mode} {name}"),
                font_size,
                max_name,
            );
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                text_x,
                cell_y,
                &display,
                font_size,
                [0.92, 0.92, 0.92, 1.0],
            );
            let ping_text = format!("{}ms", ping.max(&0));
            let color = if *ping < 150 {
                [0.45, 1.0, 0.35, 1.0]
            } else if *ping < 300 {
                [1.0, 0.85, 0.25, 1.0]
            } else {
                [1.0, 0.35, 0.25, 1.0]
            };
            context.gui.font_batch.draw_text(
                &mut context.gui.font,
                cell_x + column_width - ping_width - 4.0 * gs,
                cell_y,
                &ping_text,
                font_size * 0.86,
                color,
            );
        }
        let footer_y = list_y + rows as f32 * line_height + 4.0 * gs;
        for (index, line) in footer.iter().enumerate() {
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                metrics.sw * 0.5,
                footer_y + index as f32 * line_height,
                line,
                metrics.font_sz * 0.70,
                [0.78, 0.78, 0.78, 1.0],
            );
        }
    }

    pub(super) fn draw_sign_editor(&self, context: &mut HudContext<'_>) {
        if !self.sign_editor_open {
            return;
        }
        let metrics = hud_metrics(context);
        let gs = metrics.gs;
        context
            .gui
            .font_batch
            .fill_rect(0.0, 0.0, metrics.sw, metrics.sh, [0.0, 0.0, 0.0, 0.52]);
        let width = 160.0 * gs;
        let height = 96.0 * gs;
        let x = (metrics.sw - width) * 0.5;
        let y = (metrics.sh - height) * 0.5 - 24.0 * gs;
        context
            .gui
            .font_batch
            .fill_rect(x, y, width, height, [0.57, 0.35, 0.16, 1.0]);
        context.gui.font_batch.fill_rect(
            x + 4.0 * gs,
            y + 4.0 * gs,
            width - 8.0 * gs,
            height - 8.0 * gs,
            [0.7, 0.46, 0.22, 1.0],
        );
        context.gui.font_batch.fill_rect(
            x + width * 0.5 - 5.0 * gs,
            y + height,
            10.0 * gs,
            42.0 * gs,
            [0.33, 0.2, 0.1, 1.0],
        );
        for index in 0..4 {
            let line_y = y + 18.0 * gs + index as f32 * 17.0 * gs;
            if index == self.sign_editor_active_line {
                context.gui.font_batch.fill_rect(
                    x + 18.0 * gs,
                    line_y - 3.0 * gs,
                    width - 36.0 * gs,
                    13.0 * gs,
                    [0.0, 0.0, 0.0, 0.2],
                );
            }
            let mut text = self.sign_editor_lines[index].clone();
            if index == self.sign_editor_active_line {
                text.push('_');
            }
            context.gui.font_batch.draw_text_centered(
                &mut context.gui.font,
                x + width * 0.5,
                line_y,
                &text,
                metrics.font_sz * 0.9,
                [0.08, 0.06, 0.04, 1.0],
            );
        }
    }
}

fn wrap_line(
    font: &crate::ui::font::FontRenderer,
    text: &str,
    size: f32,
    max_width: f32,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut line = String::new();
    let mut color = String::new();
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\u{00a7}' {
            if let Some(code) = chars.next() {
                line.push(character);
                line.push(code);
                if code.eq_ignore_ascii_case(&'r') {
                    color.clear();
                } else if code.is_ascii_hexdigit() {
                    color = format!("\u{00a7}{code}");
                }
            }
            continue;
        }
        let mut candidate = line.clone();
        candidate.push(character);
        if !line.is_empty() && font.text_width(&candidate, size) > max_width {
            result.push(line);
            line = color.clone();
        }
        line.push(character);
    }
    if !line.is_empty() {
        result.push(line);
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn fit_text(font: &crate::ui::font::FontRenderer, text: &str, size: f32, max: f32) -> String {
    if font.text_width(text, size) <= max {
        return text.to_owned();
    }
    let mut output = String::new();
    for character in text.chars() {
        let candidate = format!("{output}{character}...");
        if font.text_width(&candidate, size) > max {
            break;
        }
        output.push(character);
    }
    output.push_str("...");
    output
}

fn split_lines(text: &str, max_lines: usize) -> Vec<String> {
    text.lines()
        .flat_map(|line| {
            line.chars()
                .collect::<Vec<_>>()
                .chunks(42)
                .map(|chunk| chunk.iter().collect::<String>())
                .collect::<Vec<_>>()
        })
        .filter(|line| !line.trim().is_empty())
        .take(max_lines)
        .collect()
}
