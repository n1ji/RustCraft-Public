use crate::render::gui::widgets::{
    draw_button, draw_button_enabled, draw_slider, draw_title, MenuMetrics,
};
use crate::render::gui_renderer::GuiRenderer;
use crate::render::settings::RenderSettings;

pub(crate) struct ScreenContext<'a> {
    pub metrics: MenuMetrics,
    pub in_world: bool,
    pub settings: &'a RenderSettings,
    pub gui: &'a mut GuiRenderer,
}

impl ScreenContext<'_> {
    pub fn button(&mut self, id: u32, rect: [f32; 4], label: impl AsRef<str>) {
        let gui = &mut self.gui;
        draw_button(
            &self.metrics,
            &mut gui.widget_batch,
            &mut gui.font_batch,
            id,
            rect,
            label.as_ref(),
            &mut gui.font,
        );
    }

    pub fn button_enabled(
        &mut self,
        id: u32,
        rect: [f32; 4],
        label: impl AsRef<str>,
        enabled: bool,
    ) {
        let gui = &mut self.gui;
        draw_button_enabled(
            &self.metrics,
            &mut gui.widget_batch,
            &mut gui.font_batch,
            id,
            rect,
            label.as_ref(),
            &mut gui.font,
            enabled,
        );
    }

    pub fn slider(&mut self, id: u32, rect: [f32; 4], label: impl AsRef<str>, value: f32) {
        let gui = &mut self.gui;
        draw_slider(
            &self.metrics,
            &mut gui.widget_batch,
            &mut gui.font_batch,
            id,
            rect,
            label.as_ref(),
            value,
            &mut gui.font,
        );
    }

    pub fn title(&mut self, title: impl AsRef<str>) {
        let gui = &mut self.gui;
        draw_title(
            &self.metrics,
            &mut gui.font_batch,
            &mut gui.font,
            title.as_ref(),
        );
    }

    pub fn text_center(&mut self, y: f32, text: impl AsRef<str>, size: f32, color: [f32; 4]) {
        self.text_center_at(self.metrics.sw * 0.5, y, text, size, color);
    }

    pub fn text_center_at(
        &mut self,
        x: f32,
        y: f32,
        text: impl AsRef<str>,
        size: f32,
        color: [f32; 4],
    ) {
        self.gui.font_batch.draw_text_centered(
            &mut self.gui.font,
            x,
            y,
            text.as_ref(),
            size,
            color,
        );
    }

    pub fn text(&mut self, x: f32, y: f32, text: impl AsRef<str>, size: f32, color: [f32; 4]) {
        self.gui
            .font_batch
            .draw_text(&mut self.gui.font, x, y, text.as_ref(), size, color);
    }

    pub fn panel(&mut self, rect: [f32; 4], color: [f32; 4]) {
        self.gui
            .overlay_batch
            .fill_rect(rect[0], rect[1], rect[2], rect[3], color);
    }

    pub fn text_field(&mut self, id: u32, rect: [f32; 4], value: &str, focused: bool) {
        let [x, y, width, height] = rect;
        self.gui.overlay_batch.fill_rect(
            x,
            y,
            width,
            height,
            if focused {
                [0.12, 0.12, 0.12, 0.98]
            } else {
                [0.03, 0.03, 0.03, 0.95]
            },
        );
        let border = if focused { 0.85 } else { 0.45 };
        self.gui
            .overlay_batch
            .fill_rect(x, y, width, 1.0, [border, border, border, 1.0]);
        self.gui.overlay_batch.fill_rect(
            x,
            y + height - 1.0,
            width,
            1.0,
            [border, border, border, 1.0],
        );
        self.gui
            .widget_batch
            .register_button(id, x, y, width, height);
        self.text(
            x + 4.0 * self.metrics.gs,
            y + (height - self.metrics.font_sz) * 0.5,
            value,
            self.metrics.font_sz,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    pub fn key(&self, key: &'static str) -> String {
        self.settings.ui_text.get(key).to_string()
    }
}
