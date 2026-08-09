use crate::ui::button_ids as btn;

use super::{visible_range, ScreenContext, ScreenRenderer};

enum ControlListItem {
    Category(String),
    Binding(crate::render::scene::ControlBindingRow),
}

impl ScreenRenderer {
    pub(super) fn draw_multiplayer_menu(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        context.text_center(
            20.0 * scale,
            text.get("selectServer.title"),
            12.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );

        let list_y = 32.0 * scale;
        let list_height = (metrics.sh - 96.0 * scale).max(36.0 * scale);
        context.panel(
            [0.0, list_y, metrics.sw, list_height],
            [0.0, 0.0, 0.0, 0.54],
        );
        let content_width = (306.0 * scale).min(metrics.sw - 24.0 * scale);
        let content_x = (metrics.sw - content_width) * 0.5;
        let visible = (list_height / (36.0 * scale)).floor().max(1.0) as usize;
        let range = visible_range(
            self.server_list.len(),
            &mut self.server_list_scroll,
            visible,
        );
        if self.server_list.is_empty() {
            context.text_center(
                list_y + 16.0 * scale,
                if self.server_refreshing {
                    text.get("selectServer.refresh")
                } else {
                    text.get("selectServer.empty")
                },
                metrics.font_sz,
                [0.65, 0.65, 0.65, 1.0],
            );
        }
        for index in range {
            let row = self.server_list[index].clone();
            let y = list_y + (index - self.server_list_scroll) as f32 * 36.0 * scale;
            let selected = index == self.selected_server;
            let hovered = metrics.mouse_pos[0] >= content_x
                && metrics.mouse_pos[0] <= content_x + content_width
                && metrics.mouse_pos[1] >= y
                && metrics.mouse_pos[1] <= y + 36.0 * scale;
            if selected {
                context.panel(
                    [
                        content_x - scale,
                        y,
                        content_width + 2.0 * scale,
                        36.0 * scale,
                    ],
                    [0.58, 0.58, 0.58, 1.0],
                );
                context.panel(
                    [content_x, y + scale, content_width, 34.0 * scale],
                    [0.08, 0.08, 0.08, 0.96],
                );
            } else if hovered {
                context.panel(
                    [content_x, y, content_width, 36.0 * scale],
                    [0.16, 0.16, 0.16, 0.92],
                );
            }
            context.gui.widget_batch.register_button(
                btn::SERVER_ROW_BASE + index as u32,
                content_x,
                y,
                content_width,
                36.0 * scale,
            );
            let icon_size = 32.0 * scale;
            let icon_x = content_x;
            let icon_y = y + 2.0 * scale;
            if let Some(pixels) = row.favicon_pixels.as_deref() {
                draw_favicon(context, icon_x, icon_y, icon_size, pixels);
            } else {
                context.panel(
                    [icon_x, icon_y, icon_size, icon_size],
                    [0.11, 0.11, 0.11, 1.0],
                );
                context.panel(
                    [
                        icon_x + scale,
                        icon_y + 3.0 * scale,
                        icon_size - 2.0 * scale,
                        icon_size - 2.0 * scale,
                    ],
                    [0.24, 0.24, 0.24, 1.0],
                );
            }
            let initials: String = row
                .name
                .split_whitespace()
                .filter_map(|part| part.chars().next())
                .take(2)
                .collect();
            context.text_center_at(
                content_x + icon_size * 0.5,
                y + 11.0 * scale,
                initials,
                metrics.font_sz * 0.72,
                [0.82, 0.82, 0.82, 1.0],
            );
            let text_x = content_x + 35.0 * scale;
            context.text(
                text_x,
                y + 2.0 * scale,
                &row.name,
                metrics.font_sz,
                [1.0, 1.0, 1.0, 1.0],
            );
            let detail = row
                .description
                .as_deref()
                .filter(|value| !value.is_empty())
                .or(row.error.as_deref())
                .unwrap_or(&row.address);
            context.text(
                text_x,
                y + 14.0 * scale,
                detail,
                metrics.font_sz * 0.72,
                [0.63, 0.63, 0.63, 1.0],
            );
            if detail != row.address {
                context.text(
                    text_x,
                    y + 24.0 * scale,
                    &row.address,
                    metrics.font_sz * 0.62,
                    [0.48, 0.48, 0.48, 1.0],
                );
            }
            let status = server_status_label(&text, &row);
            let status_width = context.gui.font.text_width(&status, metrics.font_sz * 0.72);
            context.text(
                content_x + content_width - status_width - 14.0 * scale,
                y + 2.0 * scale,
                status,
                metrics.font_sz * 0.72,
                if row.online {
                    [0.55, 1.0, 0.55, 1.0]
                } else {
                    [0.55, 0.28, 0.28, 1.0]
                },
            );
            draw_ping_bars(
                context,
                content_x + content_width - 10.0 * scale,
                y + 2.0 * scale,
                scale,
                row.ping_ms,
                row.online,
            );
        }

        if self.server_list.len() > visible {
            let track_y = list_y + 2.0 * scale;
            let track_h = list_height - 4.0 * scale;
            let thumb_h =
                (track_h * visible as f32 / self.server_list.len() as f32).max(8.0 * scale);
            let max_scroll = self.server_list.len().saturating_sub(visible).max(1);
            let thumb_y = track_y
                + (track_h - thumb_h)
                    * (self.server_list_scroll.min(max_scroll) as f32 / max_scroll as f32);
            context.panel(
                [metrics.sw - 4.0 * scale, track_y, 2.0 * scale, track_h],
                [0.18, 0.18, 0.18, 0.85],
            );
            context.panel(
                [metrics.sw - 4.0 * scale, thumb_y, 2.0 * scale, thumb_h],
                [0.72, 0.72, 0.72, 0.9],
            );
        }

        let has_selection =
            !self.server_list.is_empty() && self.selected_server < self.server_list.len();
        let top_button_y = metrics.sh - 52.0 * scale;
        let lower_button_y = metrics.sh - 28.0 * scale;
        context.button_enabled(
            btn::CONNECT,
            [
                metrics.sw * 0.5 - 154.0 * scale,
                top_button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("selectServer.select"),
            has_selection,
        );
        context.button(
            btn::DIRECT_CONNECT,
            [
                metrics.sw * 0.5 - 50.0 * scale,
                top_button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("selectServer.direct"),
        );
        context.button(
            btn::ADD_SERVER,
            [
                metrics.sw * 0.5 + 54.0 * scale,
                top_button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("selectServer.add"),
        );
        context.button_enabled(
            btn::EDIT_SERVER,
            [
                metrics.sw * 0.5 - 154.0 * scale,
                lower_button_y,
                70.0 * scale,
                metrics.btn_h,
            ],
            text.get("selectServer.edit"),
            has_selection,
        );
        context.button_enabled(
            btn::DELETE_SERVER,
            [
                metrics.sw * 0.5 - 74.0 * scale,
                lower_button_y,
                70.0 * scale,
                metrics.btn_h,
            ],
            text.get("selectServer.delete"),
            has_selection,
        );
        context.button(
            btn::REFRESH_SERVER_LIST,
            [
                metrics.sw * 0.5 + 4.0 * scale,
                lower_button_y,
                70.0 * scale,
                metrics.btn_h,
            ],
            text.get("selectServer.refresh"),
        );
        context.button(
            btn::MULTIPLAYER_CANCEL,
            [
                metrics.sw * 0.5 + 80.0 * scale,
                lower_button_y,
                75.0 * scale,
                metrics.btn_h,
            ],
            text.get("gui.cancel"),
        );
    }

    pub(super) fn draw_direct_connect(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        context.text_center(
            20.0 * scale,
            text.get("selectServer.direct"),
            12.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.text(
            metrics.btn_x,
            100.0 * scale,
            text.get("addServer.enterIp"),
            metrics.font_sz,
            [0.72, 0.72, 0.72, 1.0],
        );
        context.text_field(
            btn::DIRECT_ADDRESS_FIELD,
            [metrics.btn_x, 116.0 * scale, metrics.btn_w, metrics.btn_h],
            &self.server_address,
            true,
        );
        let y = metrics.sh * 0.25 + 108.0 * scale;
        context.button_enabled(
            btn::CONNECT,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            text.get("selectServer.select"),
            !self.server_address.trim().is_empty(),
        );
        context.button(
            btn::MULTIPLAYER_CANCEL,
            [
                metrics.btn_x,
                y + 24.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.cancel"),
        );
    }

    pub(super) fn draw_server_editor(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        context.text_center(
            17.0 * scale,
            text.get("addServer.title"),
            12.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        let field_x = metrics.btn_x;
        context.text(
            field_x,
            53.0 * scale,
            text.get("addServer.enterName"),
            metrics.font_sz,
            [0.72, 0.72, 0.72, 1.0],
        );
        context.text_field(
            btn::SERVER_NAME_FIELD,
            [field_x, 66.0 * scale, metrics.btn_w, metrics.btn_h],
            &self.server_editor_name,
            !self.server_editor_address_focused,
        );
        context.text(
            field_x,
            94.0 * scale,
            text.get("addServer.enterIp"),
            metrics.font_sz,
            [0.72, 0.72, 0.72, 1.0],
        );
        context.text_field(
            btn::SERVER_ADDRESS_FIELD,
            [field_x, 106.0 * scale, metrics.btn_w, metrics.btn_h],
            &self.server_editor_address,
            self.server_editor_address_focused,
        );
        let y = metrics.sh * 0.25 + 114.0 * scale;
        context.button_enabled(
            btn::SERVER_EDITOR_SAVE,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            text.get("addServer.add"),
            !self.server_editor_name.trim().is_empty()
                && !self.server_editor_address.trim().is_empty(),
        );
        context.button(
            btn::SERVER_EDITOR_CANCEL,
            [
                metrics.btn_x,
                y + 24.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.cancel"),
        );
    }

    pub(super) fn draw_controls(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let controls_gamepad = context.settings.controls_gamepad;
        let mouse_sensitivity = context.settings.mouse_sensitivity;
        let invert_mouse = context.settings.invert_mouse;
        let gamepad_look_sensitivity = context.settings.gamepad_look_sensitivity;
        let gamepad_cursor_speed = context.settings.gamepad_cursor_speed;
        let bindings = context.settings.control_bindings.clone();
        context.text_center(
            8.0 * scale,
            text.get("controls.title"),
            12.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.button(
            btn::CONTROL_DEVICE_TOGGLE,
            [
                metrics.sw * 0.5 - 75.0 * scale,
                23.0 * scale,
                150.0 * scale,
                18.0 * scale,
            ],
            if controls_gamepad {
                text.get("rustcraft.controls.controller")
            } else {
                text.get("rustcraft.controls.keyboardMouse")
            },
        );
        if controls_gamepad {
            context.slider(
                btn::GAMEPAD_LOOK_SENSITIVITY,
                [
                    metrics.sw * 0.5 - 155.0 * scale,
                    45.0 * scale,
                    150.0 * scale,
                    18.0 * scale,
                ],
                format!(
                    "{}: {}%",
                    text.get("rustcraft.controls.lookSensitivity"),
                    gamepad_look_sensitivity * 200.0
                ),
                gamepad_look_sensitivity,
            );
            context.slider(
                btn::GAMEPAD_CURSOR_SPEED,
                [
                    metrics.sw * 0.5 + 5.0 * scale,
                    45.0 * scale,
                    150.0 * scale,
                    18.0 * scale,
                ],
                format!(
                    "{}: {}%",
                    text.get("rustcraft.controls.cursorSpeed"),
                    gamepad_cursor_speed * 200.0
                ),
                gamepad_cursor_speed,
            );
        } else {
            context.slider(
                btn::MOUSE_SENSITIVITY,
                [
                    metrics.sw * 0.5 - 155.0 * scale,
                    45.0 * scale,
                    150.0 * scale,
                    18.0 * scale,
                ],
                format!(
                    "{}: {:.0}%",
                    text.get("options.sensitivity"),
                    mouse_sensitivity * 200.0
                ),
                mouse_sensitivity,
            );
            context.button(
                btn::INVERT_MOUSE_TOGGLE,
                [
                    metrics.sw * 0.5 + 5.0 * scale,
                    45.0 * scale,
                    150.0 * scale,
                    18.0 * scale,
                ],
                if invert_mouse {
                    text.get("options.invertMouse.on")
                } else {
                    text.get("options.invertMouse.off")
                },
            );
        }

        let mut items = Vec::new();
        let mut category = String::new();
        for row in bindings {
            if row.category != category {
                category = row.category.clone();
                items.push(ControlListItem::Category(category.clone()));
            }
            items.push(ControlListItem::Binding(row));
        }
        let list_y = 70.0 * scale;
        let list_height = (metrics.sh - 80.0 * scale).max(20.0 * scale);
        let row_height = 20.0 * scale;
        let visible = (list_height / row_height).floor().max(1.0) as usize;
        let range = visible_range(items.len(), &mut self.controls_list_scroll, visible);
        context.panel(
            [0.0, list_y, metrics.sw, list_height],
            [0.0, 0.0, 0.0, 0.55],
        );
        for index in range {
            let item = &items[index];
            let y = list_y + (index - self.controls_list_scroll) as f32 * row_height;
            match item {
                ControlListItem::Category(label) => {
                    context.text_center(
                        y + 7.0 * scale,
                        label,
                        metrics.font_sz * 0.8,
                        [1.0, 1.0, 1.0, 1.0],
                    );
                }
                ControlListItem::Binding(row) => {
                    let Some(action_index) = row.action.bindable_index() else {
                        continue;
                    };
                    let binding_x = metrics.sw * 0.5 - 25.0 * scale;
                    let reset_x = binding_x + 80.0 * scale;
                    let label = fit_control_text(
                        &context.gui.font,
                        &row.label,
                        metrics.font_sz,
                        (binding_x - 12.0 * scale).max(40.0 * scale),
                    );
                    let label_width = context.gui.font.text_width(&label, metrics.font_sz);
                    context.text(
                        binding_x - 5.0 * scale - label_width,
                        y + 6.0 * scale,
                        label,
                        metrics.font_sz,
                        [1.0, 1.0, 1.0, 1.0],
                    );
                    let binding_label = if row.listening {
                        format!("> {} <", row.binding)
                    } else {
                        row.binding.clone()
                    };
                    context.button(
                        btn::CONTROL_BIND_BASE + action_index as u32,
                        [binding_x, y, 75.0 * scale, 20.0 * scale],
                        binding_label,
                    );
                    if row.conflict {
                        context.panel(
                            [binding_x, y + 18.0 * scale, 75.0 * scale, 2.0 * scale],
                            [0.9, 0.18, 0.18, 1.0],
                        );
                    }
                    context.button_enabled(
                        btn::CONTROL_RESET_BASE + action_index as u32,
                        [reset_x, y, 50.0 * scale, 20.0 * scale],
                        text.get("controls.reset"),
                        !row.is_default,
                    );
                }
            }
        }
        if items.len() > visible {
            let track_y = list_y + 2.0 * scale;
            let track_h = list_height - 4.0 * scale;
            let thumb_h = (track_h * visible as f32 / items.len() as f32).max(8.0 * scale);
            let max_scroll = items.len().saturating_sub(visible).max(1);
            let thumb_y = track_y
                + (track_h - thumb_h)
                    * (self.controls_list_scroll.min(max_scroll) as f32 / max_scroll as f32);
            context.panel(
                [metrics.sw - 4.0 * scale, track_y, 2.0 * scale, track_h],
                [0.18, 0.18, 0.18, 0.85],
            );
            context.panel(
                [metrics.sw - 4.0 * scale, thumb_y, 2.0 * scale, thumb_h],
                [0.72, 0.72, 0.72, 0.9],
            );
        }
        context.button(
            btn::RESET_CONTROLS,
            [
                metrics.sw * 0.5 + 5.0 * scale,
                metrics.sh - 29.0 * scale,
                150.0 * scale,
                metrics.btn_h,
            ],
            text.get("controls.resetAll"),
        );
        context.button(
            btn::DONE,
            [
                metrics.sw * 0.5 - 155.0 * scale,
                metrics.sh - 29.0 * scale,
                150.0 * scale,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
    }

    pub(super) fn draw_alt_manager(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        context.text_center(
            18.0 * scale,
            text.get("rustcraft.altmanager.title"),
            14.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        let list_x = metrics.sw * 0.5 - 150.0 * scale;
        let list_y = 38.0 * scale;
        let list_width = (300.0 * scale).min(metrics.sw - 24.0 * scale);
        let list_x = (metrics.sw - list_width) * 0.5;
        let list_height = (metrics.sh - 92.0 * scale).max(40.0 * scale);
        context.panel([0.0, list_y, metrics.sw, list_height], [0.0, 0.0, 0.0, 0.5]);
        if self.account_list.is_empty() {
            context.text_center(
                72.0 * scale,
                text.get("rustcraft.altmanager.empty"),
                metrics.font_sz,
                [0.65, 0.65, 0.65, 1.0],
            );
        }
        let row_height = 34.0 * scale;
        let visible = (list_height / row_height).floor().max(1.0) as usize;
        let first = self.selected_account.saturating_sub(4);
        for index in first..(first + visible).min(self.account_list.len()) {
            let (name, uuid, active) = self.account_list[index].clone();
            let y = list_y + (index - first) as f32 * row_height;
            if index == self.selected_account {
                context.panel(
                    [list_x, y, list_width, 32.0 * scale],
                    [0.25, 0.25, 0.25, 0.95],
                );
            }
            context.gui.widget_batch.register_button(
                btn::ALT_ACCOUNT_ROW_BASE + index as u32,
                list_x,
                y,
                list_width,
                32.0 * scale,
            );
            if let Some(face) = self.account_faces.get(&uuid) {
                context.gui.font_batch.draw_pixel_face(
                    list_x + 4.0 * scale,
                    y + 4.0 * scale,
                    3.0 * scale,
                    face,
                );
            }
            context.text(
                list_x + 34.0 * scale,
                y + 5.0 * scale,
                name,
                metrics.font_sz,
                if active {
                    [0.45, 1.0, 0.45, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 1.0]
                },
            );
            context.text(
                list_x + 34.0 * scale,
                y + 18.0 * scale,
                uuid,
                metrics.font_sz * 0.65,
                [0.65, 0.65, 0.65, 1.0],
            );
        }
        let button_y = metrics.sh - 48.0 * scale;
        context.button_enabled(
            btn::ALT_LOGIN,
            [
                metrics.sw * 0.5 - 154.0 * scale,
                button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("rustcraft.altmanager.addAccount"),
            true,
        );
        context.button(
            btn::ALT_OFFLINE,
            [
                metrics.sw * 0.5 - 50.0 * scale,
                button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("rustcraft.altmanager.addOffline"),
        );
        let has_selection =
            !self.account_list.is_empty() && self.selected_account < self.account_list.len();
        context.button_enabled(
            btn::ALT_USE,
            [
                metrics.sw * 0.5 + 54.0 * scale,
                button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("rustcraft.altmanager.useAccount"),
            has_selection,
        );
        context.button_enabled(
            btn::ALT_LOGOUT,
            [
                metrics.sw * 0.5 + 158.0 * scale,
                button_y,
                100.0 * scale,
                metrics.btn_h,
            ],
            text.get("rustcraft.altmanager.remove"),
            has_selection,
        );
        context.button(
            btn::ALT_BACK,
            [
                metrics.btn_x,
                button_y + 24.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
        context.text_center(
            button_y - 13.0 * scale,
            &self.account_status,
            metrics.font_sz * 0.7,
            [0.7, 0.7, 0.7, 1.0],
        );

        if self.entering_offline_name {
            let overlay_y = (metrics.sh - 50.0 * scale) * 0.5;
            context.panel([0.0, 0.0, metrics.sw, metrics.sh], [0.0, 0.0, 0.0, 0.52]);
            context.panel(
                [
                    metrics.sw * 0.5 - 130.0 * scale,
                    overlay_y,
                    260.0 * scale,
                    50.0 * scale,
                ],
                [0.12, 0.12, 0.12, 0.95],
            );
            context.text_center(
                overlay_y + 8.0 * scale,
                text.get("rustcraft.altmanager.enterUsername"),
                metrics.font_sz * 0.8,
                [0.9, 0.9, 0.9, 1.0],
            );
            let input_text = if self.offline_username_input.is_empty() {
                text.get("rustcraft.altmanager.usernameHint").to_string()
            } else {
                self.offline_username_input.clone()
            };
            context.text_center(
                overlay_y + 28.0 * scale,
                input_text,
                metrics.font_sz,
                if self.offline_username_input.is_empty() {
                    [0.4, 0.4, 0.4, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 1.0]
                },
            );
            context.text_center(
                overlay_y + 56.0 * scale,
                text.get("rustcraft.altmanager.confirmHint"),
                metrics.font_sz * 0.55,
                [0.55, 0.55, 0.55, 1.0],
            );
        }
    }

    pub(super) fn draw_resource_packs(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let width = 200.0 * scale;
        let top = 32.0 * scale;
        let height = (metrics.sh - 83.0 * scale).max(36.0 * scale);
        let left_x = metrics.sw * 0.5 - 204.0 * scale;
        let right_x = metrics.sw * 0.5 + 4.0 * scale;
        context.text_center(
            16.0 * scale,
            text.get("resourcePack.title"),
            metrics.font_sz,
            [1.0, 1.0, 1.0, 1.0],
        );
        draw_pack_column(
            context,
            left_x,
            top,
            width,
            height,
            text.get("resourcePack.available.title"),
            &self.available_resource_packs,
            &mut self.available_resource_pack_scroll,
            false,
        );
        draw_pack_column(
            context,
            right_x,
            top,
            width,
            height,
            text.get("resourcePack.selected.title"),
            &self.selected_resource_packs,
            &mut self.selected_resource_pack_scroll,
            true,
        );
        context.button(
            btn::RESOURCE_PACK_OPEN_FOLDER,
            [
                metrics.sw * 0.5 - 154.0 * scale,
                metrics.sh - 48.0 * scale,
                150.0 * scale,
                metrics.btn_h,
            ],
            text.get("resourcePack.openFolder"),
        );
        context.button(
            btn::DONE,
            [
                metrics.sw * 0.5 + 4.0 * scale,
                metrics.sh - 48.0 * scale,
                150.0 * scale,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
        context.text_center_at(
            metrics.sw * 0.5 - 77.0 * scale,
            metrics.sh - 26.0 * scale,
            text.get("resourcePack.folderInfo"),
            metrics.font_sz * 0.72,
            [0.5, 0.5, 0.5, 1.0],
        );
        if let Some(status) = &self.resource_pack_status {
            context.text_center(
                metrics.sh - 16.0 * metrics.gs,
                status,
                metrics.font_sz * 0.72,
                [0.75, 0.75, 0.75, 1.0],
            );
        }
    }
}

fn fit_control_text(
    font: &crate::ui::font::FontRenderer,
    text: &str,
    size: f32,
    max: f32,
) -> String {
    if font.text_width(text, size) <= max {
        return text.to_string();
    }
    let mut output = String::new();
    for character in text.chars() {
        let mut candidate = output.clone();
        candidate.push(character);
        candidate.push_str("...");
        if font.text_width(&candidate, size) > max {
            break;
        }
        output.push(character);
    }
    output.push_str("...");
    output
}

fn draw_favicon(context: &mut ScreenContext<'_>, x: f32, y: f32, size: f32, pixels: &[u8]) {
    const SOURCE_SIZE: usize = 64;
    const SAMPLE_SIZE: usize = 16;
    let expected = SOURCE_SIZE * SOURCE_SIZE * 4;
    if pixels.len() < expected {
        context.panel([x, y, size, size], [0.11, 0.11, 0.11, 1.0]);
        return;
    }
    let pixel_size = size / SAMPLE_SIZE as f32;
    for sample_y in 0..SAMPLE_SIZE {
        for sample_x in 0..SAMPLE_SIZE {
            let source_x = sample_x * SOURCE_SIZE / SAMPLE_SIZE;
            let source_y = sample_y * SOURCE_SIZE / SAMPLE_SIZE;
            let index = (source_y * SOURCE_SIZE + source_x) * 4;
            let alpha = pixels[index + 3] as f32 / 255.0;
            if alpha > 0.01 {
                context.gui.overlay_batch.fill_rect(
                    x + sample_x as f32 * pixel_size,
                    y + sample_y as f32 * pixel_size,
                    pixel_size,
                    pixel_size,
                    [
                        pixels[index] as f32 / 255.0,
                        pixels[index + 1] as f32 / 255.0,
                        pixels[index + 2] as f32 / 255.0,
                        alpha,
                    ],
                );
            }
        }
    }
}

fn draw_ping_bars(
    context: &mut ScreenContext<'_>,
    x: f32,
    y: f32,
    scale: f32,
    ping_ms: Option<u32>,
    online: bool,
) {
    let strength = if !online {
        0
    } else {
        match ping_ms.unwrap_or(u32::MAX) {
            0..=149 => 5,
            150..=299 => 4,
            300..=599 => 3,
            600..=999 => 2,
            _ => 1,
        }
    };
    for bar in 0..5 {
        let height = (bar + 1) as f32 * 1.4 * scale;
        context.gui.overlay_batch.fill_rect(
            x + bar as f32 * 1.8 * scale,
            y + 8.0 * scale - height,
            1.2 * scale,
            height,
            if bar < strength {
                [0.65, 0.85, 0.42, 1.0]
            } else {
                [0.25, 0.25, 0.25, 1.0]
            },
        );
    }
}

fn server_status_label(
    text: &crate::ui::text::UiText,
    server: &crate::render::scene::ServerListRow,
) -> String {
    if server.online {
        match (server.players_online, server.players_max) {
            (Some(online), Some(max)) => format!("{online}/{max}"),
            _ => text.get("rustcraft.multiplayer.online").to_string(),
        }
    } else if server.error.is_some() {
        text.get("rustcraft.multiplayer.offline").to_string()
    } else {
        text.get("rustcraft.multiplayer.unknown").to_string()
    }
}

fn draw_pack_column(
    context: &mut ScreenContext<'_>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    title: impl AsRef<str>,
    packs: &[crate::client::app::ResourcePackInfo],
    scroll: &mut usize,
    selected: bool,
) {
    let metrics = context.metrics;
    let scale = metrics.gs;
    context.panel([x, y, width, height], [0.0, 0.0, 0.0, 0.58]);
    context.panel([x, y, width, 1.0 * scale], [0.5, 0.5, 0.5, 0.5]);
    context.panel(
        [x, y + height - 1.0 * scale, width, 1.0 * scale],
        [0.5, 0.5, 0.5, 0.5],
    );
    context.text_center_at(
        x + width * 0.5,
        y + 3.0 * scale,
        title,
        metrics.font_sz,
        [1.0, 1.0, 1.0, 1.0],
    );
    let list_top = y + 16.0 * scale;
    let list_height = height - 16.0 * scale;
    let row_height = 36.0 * scale;
    let visible = (list_height / row_height).floor().max(1.0) as usize;
    let range = visible_range(packs.len(), scroll, visible);
    for index in range {
        let row_y = list_top + (index - *scroll) as f32 * row_height;
        let pack = &packs[index];
        draw_pack_entry(
            context,
            if selected {
                btn::RESOURCE_PACK_SELECTED_BASE + index as u32
            } else {
                btn::RESOURCE_PACK_BASE + index as u32
            },
            x + 2.0 * scale,
            row_y,
            width - 7.0 * scale,
            pack,
            !selected,
            selected && !pack.is_default && index > 0,
            selected && !pack.is_default && index + 1 < packs.len() && !packs[index + 1].is_default,
        );
    }
    if packs.len() > visible {
        let track_y = list_top + 1.0 * scale;
        let track_h = list_height - 2.0 * scale;
        let thumb_h = (track_h * visible as f32 / packs.len() as f32).max(8.0 * scale);
        let max_scroll = packs.len().saturating_sub(visible).max(1);
        let thumb_y =
            track_y + (track_h - thumb_h) * (*scroll).min(max_scroll) as f32 / max_scroll as f32;
        context.panel(
            [x + width - 4.0 * scale, track_y, 2.0 * scale, track_h],
            [0.18, 0.18, 0.18, 0.85],
        );
        context.panel(
            [x + width - 4.0 * scale, thumb_y, 2.0 * scale, thumb_h],
            [0.72, 0.72, 0.72, 0.9],
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_pack_entry(
    context: &mut ScreenContext<'_>,
    id: u32,
    x: f32,
    y: f32,
    width: f32,
    pack: &crate::client::app::ResourcePackInfo,
    available: bool,
    can_move_up: bool,
    can_move_down: bool,
) {
    let metrics = context.metrics;
    let scale = metrics.gs;
    let row_height = 36.0 * scale;
    let hovered = metrics.mouse_pos[0] >= x
        && metrics.mouse_pos[0] <= x + 32.0 * scale
        && metrics.mouse_pos[1] >= y
        && metrics.mouse_pos[1] <= y + row_height;
    let compatible = pack.pack_format == 0 || pack.pack_format == 1;
    if !compatible {
        context.panel(
            [x - scale, y - scale, width + 2.0 * scale, 34.0 * scale],
            [0.47, 0.0, 0.0, 1.0],
        );
    }
    if !pack.is_default {
        if available {
            context
                .gui
                .widget_batch
                .register_button(id, x, y, 32.0 * scale, 32.0 * scale);
        } else {
            context
                .gui
                .widget_batch
                .register_button(id, x, y, 16.0 * scale, 32.0 * scale);
            let index = id - btn::RESOURCE_PACK_SELECTED_BASE;
            if can_move_up {
                context.gui.widget_batch.register_button(
                    btn::RESOURCE_PACK_SELECTED_UP_BASE + index,
                    x + 16.0 * scale,
                    y,
                    16.0 * scale,
                    16.0 * scale,
                );
            }
            if can_move_down {
                context.gui.widget_batch.register_button(
                    btn::RESOURCE_PACK_SELECTED_DOWN_BASE + index,
                    x + 16.0 * scale,
                    y + 16.0 * scale,
                    16.0 * scale,
                    16.0 * scale,
                );
            }
        }
    }
    context.panel([x, y, 32.0 * scale, 32.0 * scale], [0.08, 0.08, 0.08, 1.0]);
    if let Some(icon) = &pack.icon {
        draw_pack_icon(context, x, y, 32.0 * scale, icon);
    }
    if hovered {
        context.panel([x, y, 32.0 * scale, 32.0 * scale], [0.0, 0.0, 0.0, 0.55]);
        if available {
            draw_pack_control(context, x + 16.0 * scale, y + 10.0 * scale, ">", scale);
        } else if !pack.is_default {
            draw_pack_control(context, x + 8.0 * scale, y + 10.0 * scale, "<", scale);
            if can_move_up {
                draw_pack_control(context, x + 24.0 * scale, y + 2.0 * scale, "^", scale);
            }
            if can_move_down {
                draw_pack_control(context, x + 24.0 * scale, y + 18.0 * scale, "v", scale);
            }
        }
    }
    let text_x = x + 34.0 * scale;
    let text_width = 157.0 * scale;
    let name = fit_text(&context.gui.font, &pack.name, metrics.font_sz, text_width);
    context.text(
        text_x,
        y + scale,
        name,
        metrics.font_sz,
        [1.0, 1.0, 1.0, 1.0],
    );
    let description = if compatible {
        pack.description.as_str()
    } else {
        "Incompatible with Minecraft 1.8.9"
    };
    for (line, description) in wrap_text(
        &context.gui.font,
        description,
        metrics.font_sz * 0.72,
        text_width,
    )
    .into_iter()
    .take(2)
    .enumerate()
    {
        context.text(
            text_x,
            y + (13 + 10 * line) as f32 * scale,
            description,
            metrics.font_sz * 0.72,
            [0.5, 0.5, 0.5, 1.0],
        );
    }
}

fn draw_pack_icon(context: &mut ScreenContext<'_>, x: f32, y: f32, size: f32, pixels: &[u8; 4096]) {
    let pixel_size = size / 32.0;
    for py in 0..32 {
        for px in 0..32 {
            let index = (py * 32 + px) * 4;
            let alpha = pixels[index + 3] as f32 / 255.0;
            if alpha > 0.01 {
                context.gui.overlay_batch.fill_rect(
                    x + px as f32 * pixel_size,
                    y + py as f32 * pixel_size,
                    pixel_size,
                    pixel_size,
                    [
                        pixels[index] as f32 / 255.0,
                        pixels[index + 1] as f32 / 255.0,
                        pixels[index + 2] as f32 / 255.0,
                        alpha,
                    ],
                );
            }
        }
    }
}

fn draw_pack_control(context: &mut ScreenContext<'_>, x: f32, y: f32, symbol: &str, scale: f32) {
    context.text_center_at(x, y, symbol, 12.0 * scale, [1.0, 1.0, 1.0, 1.0]);
}

fn wrap_text(
    font: &crate::ui::font::FontRenderer,
    text: &str,
    size: f32,
    max_width: f32,
) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines().filter(|line| !line.is_empty()) {
        let mut line = String::new();
        for character in paragraph.chars() {
            let mut candidate = line.clone();
            candidate.push(character);
            if !line.is_empty() && font.text_width(&candidate, size) > max_width {
                lines.push(line);
                line = String::new();
            }
            line.push(character);
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn fit_text(font: &crate::ui::font::FontRenderer, text: &str, size: f32, max_width: f32) -> String {
    if font.text_width(text, size) <= max_width {
        return text.to_string();
    }
    let suffix = "...";
    let suffix_width = font.text_width(suffix, size);
    let mut output = String::new();
    for character in text.chars() {
        let mut candidate = output.clone();
        candidate.push(character);
        if font.text_width(&candidate, size) + suffix_width > max_width {
            break;
        }
        output.push(character);
    }
    output.push_str(suffix);
    output
}
