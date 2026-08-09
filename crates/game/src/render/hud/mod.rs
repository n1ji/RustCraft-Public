use crate::render::gui_renderer::GuiRenderer;

mod chat;
mod inventory;
mod overlays;
mod render;
mod state;
mod status;

pub(super) struct HudContext<'a> {
    pub gui: &'a mut GuiRenderer,
    pub settings: &'a crate::render::settings::RenderSettings,
    pub frame: &'a crate::render::frame::FrameRenderer,
    pub world: &'a crate::render::world::WorldRenderer,
    pub screens: &'a crate::render::screens::ScreenRenderer,
    pub entity: &'a crate::render::entity_renderer::EntityRenderer,
}

/// One line of an item tooltip. The HUD keeps color information internally;
/// HudRenderer::hovered_tooltip stays a plain string list so the client layer
/// can feed pre-rendered tooltips without knowing about palette details.
#[derive(Clone, Debug)]
pub(super) struct TooltipLine {
    pub text: String,
    pub color: [f32; 4],
}

pub use state::HudRenderer;
