use nalgebra::{Matrix3, Vector3};

use crate::client::inventory::ItemStackView;
use crate::net::nbt::NbtTag;
use crate::render::gui::widgets::{draw_button, MenuMetrics};
use crate::render::hooks::ScriptDrawCommand;
use crate::ui::format::format_text;
use std::collections::HashMap;

use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
impl HudRenderer {
    pub(crate) fn draw_script_hud_before(&self, context: &mut HudContext<'_>) {
        let commands = self.script_hud_before_commands.clone();
        draw_script_commands(context, &commands);
    }

    pub(crate) fn draw_script_hud(&self, context: &mut HudContext<'_>) {
        let commands = self.script_hud_commands.clone();
        draw_script_commands(context, &commands);
    }
}

fn draw_script_commands(context: &mut HudContext<'_>, commands: &[ScriptDrawCommand]) {
    let metrics = hud_metrics(context);
    let mut transform = Matrix3::<f32>::identity();
    let mut stack = Vec::new();
    let mut scissor: Option<[f32; 4]> = None;

    for command in commands {
        match command {
            ScriptDrawCommand::PushTransform => stack.push(transform),
            ScriptDrawCommand::PopTransform => {
                if let Some(previous) = stack.pop() {
                    transform = previous;
                }
            }
            ScriptDrawCommand::Translate { x, y } => {
                transform = Matrix3::new(1.0, 0.0, *x, 0.0, 1.0, *y, 0.0, 0.0, 1.0) * transform;
            }
            ScriptDrawCommand::Rotate { degrees } => {
                let (sin, cos) = degrees.to_radians().sin_cos();
                transform = Matrix3::new(cos, -sin, 0.0, sin, cos, 0.0, 0.0, 0.0, 1.0) * transform;
            }
            ScriptDrawCommand::Scale { x, y } => {
                transform = Matrix3::new(*x, 0.0, 0.0, 0.0, *y, 0.0, 0.0, 0.0, 1.0) * transform;
            }
            ScriptDrawCommand::SetScissor(rect) => scissor = *rect,
            ScriptDrawCommand::Text {
                text,
                x,
                y,
                scale,
                color,
            } => {
                let point = transform_point(&transform, *x, *y);
                if point_visible(point, scissor) {
                    let scale_factor = transform_scale(&transform);
                    context.gui.font_batch.draw_text(
                        &mut context.gui.font,
                        point[0],
                        point[1],
                        text,
                        9.0 * metrics.gs * scale * scale_factor,
                        color.array(),
                    );
                }
            }
            ScriptDrawCommand::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                let points = transformed_quad(&transform, *x, *y, *width, *height);
                if points.iter().any(|point| point_visible(*point, scissor)) {
                    context.gui.font_batch.fill_quad(
                        points[0][0],
                        points[0][1],
                        points[1][0],
                        points[1][1],
                        points[2][0],
                        points[2][1],
                        points[3][0],
                        points[3][1],
                        color.array(),
                    );
                }
            }
            ScriptDrawCommand::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                color,
            } => {
                let from = transform_point(&transform, *x1, *y1);
                let to = transform_point(&transform, *x2, *y2);
                if point_visible(from, scissor) || point_visible(to, scissor) {
                    context.gui.font_batch.draw_line(
                        from,
                        to,
                        width * transform_scale(&transform),
                        color.array(),
                    );
                }
            }
            ScriptDrawCommand::Crosshair {
                x_offset,
                y_offset,
                size,
                gap,
                thickness,
                color,
            } => {
                let cx = context.gui.viewport[0].max(1) as f32 * 0.5 + *x_offset;
                let cy = context.gui.viewport[1].max(1) as f32 * 0.5 + *y_offset;
                let half_thickness = *thickness * 0.5;
                let color = color.array();
                context.gui.font_batch.fill_rect(
                    cx - gap - size,
                    cy - half_thickness,
                    *size,
                    *thickness,
                    color,
                );
                context.gui.font_batch.fill_rect(
                    cx + gap,
                    cy - half_thickness,
                    *size,
                    *thickness,
                    color,
                );
                context.gui.font_batch.fill_rect(
                    cx - half_thickness,
                    cy - gap - size,
                    *thickness,
                    *size,
                    color,
                );
                context.gui.font_batch.fill_rect(
                    cx - half_thickness,
                    cy + gap,
                    *thickness,
                    *size,
                    color,
                );
            }
            ScriptDrawCommand::Image {
                resource,
                x,
                y,
                width,
                height,
                color,
            } => {
                let points = transformed_quad(&transform, *x, *y, *width, *height);
                if !points.iter().any(|point| point_visible(*point, scissor)) {
                    continue;
                }
                match resource.as_str() {
                    "minecraft:textures/gui/widgets.png" => {
                        context.gui.widget_batch.textured_quad(
                            points,
                            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                            color.array(),
                        );
                    }
                    "minecraft:textures/gui/icons.png" => {
                        context.gui.icons_batch.textured_quad(
                            points,
                            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                            color.array(),
                        );
                    }
                    "minecraft:textures/gui/container/inventory.png" => {
                        context.gui.inventory_batch.textured_quad(
                            points,
                            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                            color.array(),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn transform_point(transform: &Matrix3<f32>, x: f32, y: f32) -> [f32; 2] {
    let point = transform * Vector3::new(x, y, 1.0);
    [point.x, point.y]
}

fn transformed_quad(
    transform: &Matrix3<f32>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> [[f32; 2]; 4] {
    [
        transform_point(transform, x, y),
        transform_point(transform, x + width, y),
        transform_point(transform, x + width, y + height),
        transform_point(transform, x, y + height),
    ]
}

fn transform_scale(transform: &Matrix3<f32>) -> f32 {
    let x = (transform[(0, 0)].powi(2) + transform[(1, 0)].powi(2)).sqrt();
    let y = (transform[(0, 1)].powi(2) + transform[(1, 1)].powi(2)).sqrt();
    ((x + y) * 0.5).clamp(0.01, 100.0)
}

fn point_visible(point: [f32; 2], scissor: Option<[f32; 4]>) -> bool {
    let Some([x, y, width, height]) = scissor else {
        return true;
    };
    point[0] >= x && point[0] <= x + width && point[1] >= y && point[1] <= y + height
}

// ---------------------------------------------------------------------------
// Debug panel
// ---------------------------------------------------------------------------
