use crate::render::scene::ShaderOptionRowKind;
use crate::ui::button_ids as btn;

use super::{visible_range, ScreenContext, ScreenRenderer};

impl ScreenRenderer {
    pub(super) fn draw_modding(&mut self, context: &mut ScreenContext<'_>) {
        context.title(context.key("rustcraft.modding.title"));
        let metrics = context.metrics;
        let width = (420.0 * metrics.gs).min(metrics.sw - 24.0 * metrics.gs);
        let x = (metrics.sw - width) * 0.5;
        let y = 38.0 * metrics.gs;
        let height = (metrics.sh - 126.0 * metrics.gs).max(48.0 * metrics.gs);
        context.panel([x, y, width, height], [0.0, 0.0, 0.0, 0.58]);
        let row_height = 38.0 * metrics.gs;
        let visible = (height / row_height).floor().max(1.0) as usize;
        let rows = self.modding_rows.clone();
        let range = visible_range(rows.len(), &mut self.modding_scroll, visible);
        if rows.is_empty() {
            context.text_center(
                y + 18.0 * metrics.gs,
                context.key("rustcraft.modding.empty"),
                metrics.font_sz,
                [0.68, 0.68, 0.68, 1.0],
            );
        }
        for index in range {
            let row = &rows[index];
            let row_y = y + (index - self.modding_scroll) as f32 * row_height;
            context.panel(
                [
                    x + 3.0 * metrics.gs,
                    row_y + 2.0 * metrics.gs,
                    width - 6.0 * metrics.gs,
                    34.0 * metrics.gs,
                ],
                if index == self.modding_selected {
                    [0.20, 0.24, 0.34, 0.96]
                } else {
                    [0.07, 0.07, 0.07, 0.85]
                },
            );
            context.gui.widget_batch.register_button(
                btn::MODDING_ROW_BASE + index as u32,
                x + 3.0 * metrics.gs,
                row_y + 2.0 * metrics.gs,
                width - 6.0 * metrics.gs,
                34.0 * metrics.gs,
            );
            context.text(
                x + 9.0 * metrics.gs,
                row_y + 6.0 * metrics.gs,
                format!("{}  {}", row.name, row.version),
                metrics.font_sz * 0.86,
                if row.enabled {
                    [0.55, 1.0, 0.55, 1.0]
                } else {
                    [0.65, 0.65, 0.65, 1.0]
                },
            );
            context.text(
                x + 9.0 * metrics.gs,
                row_y + 20.0 * metrics.gs,
                format!(
                    "{}{} · {} config option(s)",
                    row.id,
                    if row.protocol_translator {
                        " · protocol"
                    } else {
                        ""
                    },
                    row.config_entries
                ),
                metrics.font_sz * 0.62,
                [0.65, 0.65, 0.65, 1.0],
            );
        }

        if let Some(row) = rows.get(self.modding_selected) {
            let detail = if row.denied_permissions.is_empty() {
                format!("Granted: {}", row.granted_permissions.join(", "))
            } else {
                format!("Denied: {}", row.denied_permissions.join(", "))
            };
            context.text_center(
                metrics.sh - 80.0 * metrics.gs,
                detail,
                metrics.font_sz * 0.65,
                [0.72, 0.72, 0.72, 1.0],
            );
        }
        context.text_center(
            metrics.sh - 66.0 * metrics.gs,
            &self.modding_status,
            metrics.font_sz * 0.68,
            [0.65, 0.8, 0.82, 1.0],
        );
        let selected = rows.get(self.modding_selected);
        let locked =
            selected.is_some_and(|row| self.modding_connection_active && row.protocol_translator);
        let third = (width - 8.0 * metrics.gs) / 3.0;
        let button_y = metrics.sh - 48.0 * metrics.gs;
        context.button_enabled(
            btn::MODDING_TOGGLE,
            [x, button_y, third, metrics.btn_h],
            if selected.is_some_and(|row| row.enabled) {
                "Disable"
            } else {
                "Enable"
            },
            selected.is_some() && !locked,
        );
        context.button_enabled(
            btn::MODDING_RELOAD,
            [x + third + 4.0 * metrics.gs, button_y, third, metrics.btn_h],
            "Reload",
            selected.is_some() && !locked,
        );
        context.button_enabled(
            btn::MODDING_CONFIGURE,
            [
                x + (third + 4.0 * metrics.gs) * 2.0,
                button_y,
                third,
                metrics.btn_h,
            ],
            "Configure",
            selected.is_some_and(|row| row.config_entries > 0),
        );
        context.button(
            btn::MODDING_BACK,
            [
                metrics.btn_x,
                metrics.sh - 23.0 * metrics.gs,
                metrics.btn_w,
                18.0 * metrics.gs,
            ],
            context.key("gui.back"),
        );
    }

    pub(super) fn draw_mod_config(&mut self, context: &mut ScreenContext<'_>) {
        context.title(
            self.mod_config_title
                .clone()
                .unwrap_or_else(|| String::from("Mod Configuration")),
        );
        let metrics = context.metrics;
        let width = (440.0 * metrics.gs).min(metrics.sw - 24.0 * metrics.gs);
        let x = (metrics.sw - width) * 0.5;
        let top = 38.0 * metrics.gs;
        let height = (metrics.sh - 92.0 * metrics.gs).max(48.0 * metrics.gs);
        context.panel([x, top, width, height], [0.0, 0.0, 0.0, 0.58]);
        let rows = self.mod_config_rows.clone();
        let row_height = 48.0 * metrics.gs;
        let visible = (height / row_height).floor().max(1.0) as usize;
        let range = visible_range(rows.len(), &mut self.mod_config_scroll, visible);
        for index in range {
            let row = &rows[index];
            let y = top + (index - self.mod_config_scroll) as f32 * row_height;
            context.panel(
                [
                    x + 3.0 * metrics.gs,
                    y + 2.0 * metrics.gs,
                    width - 6.0 * metrics.gs,
                    44.0 * metrics.gs,
                ],
                if index == self.mod_config_selected {
                    [0.18, 0.22, 0.32, 0.96]
                } else {
                    [0.07, 0.07, 0.07, 0.86]
                },
            );
            context.gui.widget_batch.register_button(
                btn::MOD_CONFIG_ROW_BASE + index as u32,
                x + 3.0 * metrics.gs,
                y + 2.0 * metrics.gs,
                width - 6.0 * metrics.gs,
                44.0 * metrics.gs,
            );
            context.text(
                x + 9.0 * metrics.gs,
                y + 6.0 * metrics.gs,
                &row.label,
                metrics.font_sz * 0.85,
                [1.0, 1.0, 1.0, 1.0],
            );
            context.text(
                x + 9.0 * metrics.gs,
                y + 20.0 * metrics.gs,
                &row.description,
                metrics.font_sz * 0.6,
                [0.62, 0.62, 0.62, 1.0],
            );
            let control_width = 136.0 * metrics.gs;
            let control_x = x + width - control_width - 8.0 * metrics.gs;
            context.button_enabled(
                btn::MOD_CONFIG_PREVIOUS_BASE + index as u32,
                [
                    control_x,
                    y + 12.0 * metrics.gs,
                    24.0 * metrics.gs,
                    20.0 * metrics.gs,
                ],
                "<",
                row.can_previous && !self.mod_config_locked,
            );
            context.button_enabled(
                btn::MOD_CONFIG_NEXT_BASE + index as u32,
                [
                    control_x + control_width - 24.0 * metrics.gs,
                    y + 12.0 * metrics.gs,
                    24.0 * metrics.gs,
                    20.0 * metrics.gs,
                ],
                ">",
                row.can_next && !self.mod_config_locked,
            );
            context.text(
                control_x + 30.0 * metrics.gs,
                y + 18.0 * metrics.gs,
                &row.value,
                metrics.font_sz * 0.7,
                [0.85, 0.85, 0.85, 1.0],
            );
        }
        context.text_center(
            metrics.sh - 47.0 * metrics.gs,
            &self.mod_config_status,
            metrics.font_sz * 0.68,
            [0.68, 0.75, 0.75, 1.0],
        );
        let half = (width - 4.0 * metrics.gs) * 0.5;
        context.button_enabled(
            btn::MOD_CONFIG_RESET_ALL,
            [x, metrics.sh - 32.0 * metrics.gs, half, metrics.btn_h],
            "Reset All",
            rows.iter().any(|row| !row.is_default) && !self.mod_config_locked,
        );
        context.button(
            btn::MOD_CONFIG_BACK,
            [
                x + half + 4.0 * metrics.gs,
                metrics.sh - 32.0 * metrics.gs,
                half,
                metrics.btn_h,
            ],
            context.key("gui.done"),
        );
    }

    pub(super) fn draw_shader_packs(&mut self, context: &mut ScreenContext<'_>) {
        context.title("Shader Packs");
        let metrics = context.metrics;
        let width = 330.0 * metrics.gs;
        let x = (metrics.sw - width) * 0.5;
        let top = 38.0 * metrics.gs;
        let height = (metrics.sh - 112.0 * metrics.gs).max(48.0 * metrics.gs);
        context.panel([x, top, width, height], [0.0, 0.0, 0.0, 0.58]);
        let packs = self.shader_packs.clone();
        let row_height = 36.0 * metrics.gs;
        let visible = (height / row_height).floor().max(1.0) as usize;
        let range = visible_range(packs.len(), &mut self.shader_pack_scroll, visible);
        for index in range {
            let pack = &packs[index];
            let y = top + (index - self.shader_pack_scroll) as f32 * row_height;
            let selected = self.selected_shader_pack.as_deref() == Some(&pack.source_name);
            context.panel(
                [
                    x + 3.0 * metrics.gs,
                    y + 2.0 * metrics.gs,
                    width - 6.0 * metrics.gs,
                    32.0 * metrics.gs,
                ],
                if selected {
                    [0.16, 0.36, 0.16, 0.94]
                } else if !pack.compatible {
                    [0.36, 0.08, 0.08, 0.94]
                } else {
                    [0.07, 0.07, 0.07, 0.86]
                },
            );
            if pack.compatible {
                context.gui.widget_batch.register_button(
                    btn::SHADER_PACK_BASE + index as u32,
                    x + 3.0 * metrics.gs,
                    y + 2.0 * metrics.gs,
                    width - 6.0 * metrics.gs,
                    32.0 * metrics.gs,
                );
            }
            context.text(
                x + 9.0 * metrics.gs,
                y + 6.0 * metrics.gs,
                format!("{} [{}]", pack.name, pack.format.label()),
                metrics.font_sz * 0.82,
                [1.0, 1.0, 1.0, 1.0],
            );
            context.text(
                x + 9.0 * metrics.gs,
                y + 19.0 * metrics.gs,
                pack.error.as_deref().unwrap_or(&pack.description),
                metrics.font_sz * 0.62,
                if pack.compatible {
                    [0.65, 0.65, 0.65, 1.0]
                } else {
                    [1.0, 0.4, 0.4, 1.0]
                },
            );
        }
        context.text_center(
            metrics.sh - 66.0 * metrics.gs,
            &self.shader_pack_status,
            metrics.font_sz * 0.68,
            [0.68, 0.72, 0.72, 1.0],
        );
        let third = (width - 8.0 * metrics.gs) / 3.0;
        let y = metrics.sh - 50.0 * metrics.gs;
        context.button(btn::SHADER_PACK_OFF, [x, y, third, metrics.btn_h], "OFF");
        context.button(
            btn::SHADER_PACK_OPEN_FOLDER,
            [x + third + 4.0 * metrics.gs, y, third, metrics.btn_h],
            "Open Folder",
        );
        context.button_enabled(
            btn::SHADER_OPTIONS,
            [
                x + (third + 4.0 * metrics.gs) * 2.0,
                y,
                third,
                metrics.btn_h,
            ],
            "Options",
            self.selected_shader_pack.is_some(),
        );
        context.button(
            btn::DONE,
            [
                metrics.btn_x,
                metrics.sh - 24.0 * metrics.gs,
                metrics.btn_w,
                18.0 * metrics.gs,
            ],
            context.key("gui.done"),
        );
    }

    pub(super) fn draw_shader_options(&mut self, context: &mut ScreenContext<'_>) {
        context.title(if self.shader_option_title.is_empty() {
            String::from("Shader Options")
        } else {
            self.shader_option_title.clone()
        });
        let metrics = context.metrics;
        let width = (430.0 * metrics.gs).min(metrics.sw - 24.0 * metrics.gs);
        let x = (metrics.sw - width) * 0.5;
        let top = 38.0 * metrics.gs;
        let height = (metrics.sh - 92.0 * metrics.gs).max(48.0 * metrics.gs);
        context.panel([x, top, width, height], [0.0, 0.0, 0.0, 0.58]);
        let rows = self.shader_option_rows.clone();
        let row_height = 38.0 * metrics.gs;
        let visible = (height / row_height).floor().max(1.0) as usize;
        let range = visible_range(rows.len(), &mut self.shader_option_scroll, visible);
        for index in range {
            let row = &rows[index];
            let y = top + (index - self.shader_option_scroll) as f32 * row_height;
            if row.kind == ShaderOptionRowKind::Spacer {
                continue;
            }
            context.panel(
                [
                    x + 3.0 * metrics.gs,
                    y + 2.0 * metrics.gs,
                    width - 6.0 * metrics.gs,
                    34.0 * metrics.gs,
                ],
                [0.07, 0.07, 0.07, 0.86],
            );
            context.gui.widget_batch.register_button(
                btn::SHADER_OPTION_ROW_BASE + index as u32,
                x + 3.0 * metrics.gs,
                y + 2.0 * metrics.gs,
                width - 6.0 * metrics.gs,
                34.0 * metrics.gs,
            );
            context.text(
                x + 9.0 * metrics.gs,
                y + 6.0 * metrics.gs,
                &row.label,
                metrics.font_sz * 0.82,
                [1.0, 1.0, 1.0, 1.0],
            );
            context.text(
                x + 9.0 * metrics.gs,
                y + 19.0 * metrics.gs,
                &row.description,
                metrics.font_sz * 0.6,
                [0.62, 0.62, 0.62, 1.0],
            );
            if row.kind == ShaderOptionRowKind::Option || row.kind == ShaderOptionRowKind::Profile {
                let control_x = x + width - 142.0 * metrics.gs;
                context.button_enabled(
                    btn::SHADER_OPTION_PREVIOUS_BASE + index as u32,
                    [
                        control_x,
                        y + 8.0 * metrics.gs,
                        24.0 * metrics.gs,
                        20.0 * metrics.gs,
                    ],
                    "<",
                    row.can_previous,
                );
                context.button_enabled(
                    btn::SHADER_OPTION_NEXT_BASE + index as u32,
                    [
                        control_x + 112.0 * metrics.gs,
                        y + 8.0 * metrics.gs,
                        24.0 * metrics.gs,
                        20.0 * metrics.gs,
                    ],
                    ">",
                    row.can_next,
                );
                context.text(
                    control_x + 30.0 * metrics.gs,
                    y + 14.0 * metrics.gs,
                    &row.value,
                    metrics.font_sz * 0.68,
                    [0.85, 0.85, 0.85, 1.0],
                );
            }
        }
        context.text_center(
            metrics.sh - 47.0 * metrics.gs,
            &self.shader_option_status,
            metrics.font_sz * 0.68,
            [0.68, 0.75, 0.75, 1.0],
        );
        let half = (width - 4.0 * metrics.gs) * 0.5;
        context.button_enabled(
            btn::SHADER_OPTIONS_RESET_ALL,
            [x, metrics.sh - 32.0 * metrics.gs, half, metrics.btn_h],
            "Reset All",
            rows.iter().any(|row| !row.is_default),
        );
        context.button(
            btn::SHADER_OPTIONS_BACK,
            [
                x + half + 4.0 * metrics.gs,
                metrics.sh - 32.0 * metrics.gs,
                half,
                metrics.btn_h,
            ],
            context.key("gui.done"),
        );
    }

    pub(super) fn draw_shader_pack_loading(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        context.text_center(
            metrics.sh * 0.5 - 28.0 * metrics.gs,
            "Loading Shader Pack",
            15.0 * metrics.gs,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.text_center(
            metrics.sh * 0.5,
            &self.shader_pack_loading_name,
            metrics.font_sz,
            [0.75, 0.85, 1.0, 1.0],
        );
        context.text_center(
            metrics.sh * 0.5 + 18.0 * metrics.gs,
            &self.shader_pack_loading_stage,
            metrics.font_sz * 0.75,
            [0.68, 0.68, 0.68, 1.0],
        );
        let progress_width = 220.0 * metrics.gs;
        context.panel(
            [
                (metrics.sw - progress_width) * 0.5,
                metrics.sh * 0.5 + 38.0 * metrics.gs,
                progress_width,
                4.0 * metrics.gs,
            ],
            [0.18, 0.18, 0.18, 1.0],
        );
        let phase = (self.shader_pack_loading_stage.len() % 11) as f32 / 10.0;
        context.panel(
            [
                (metrics.sw - progress_width) * 0.5,
                metrics.sh * 0.5 + 38.0 * metrics.gs,
                progress_width * phase.max(0.12),
                4.0 * metrics.gs,
            ],
            [0.3, 0.65, 1.0, 1.0],
        );
    }
}
