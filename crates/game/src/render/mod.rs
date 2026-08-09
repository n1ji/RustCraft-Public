//! Game-facing render systems.
//!
//! Native backend ownership lives in the renderer backend crate.
//! This module keeps gameplay-facing models, GUI builders, HUD logic and
//! shader-pack descriptions available without defining a second renderer.

pub(crate) mod custom_sky;
pub mod entity;
pub mod entity_renderer;
pub mod first_person;
pub mod frame;
pub mod gui;
pub mod gui_renderer;
pub mod hooks;
pub mod hud;
pub(crate) mod item_icons;
pub mod mesh;
pub(crate) mod particle_mesh;
pub(crate) mod particles;
pub mod renderer;
mod runtime;
pub mod scene;
pub mod screens;
pub mod settings;
pub mod shader_pack;
pub mod shaders;
pub(crate) mod skin;
pub mod sky;
pub mod world;

pub use frame::{FrameDebugProfile, FrameRenderer, FrameSpikeBreakdown};
pub use particles::ParticleSprite;
pub use renderer::GameRenderer;
pub use settings::RenderSettings;

use renderer_api::core::{DrawPacket, RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{BindGroupHandle, BufferHandle, MaterialHandle, PipelineHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshDraw {
    pub pipeline: PipelineHandle,
    pub bind_group: Option<BindGroupHandle>,
    pub vertex_buffer: BufferHandle,
    pub index_buffer: Option<BufferHandle>,
    pub material: Option<MaterialHandle>,
    pub push_constants: Option<[u8; 16]>,
    pub index_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub sort_key: u64,
}

impl MeshDraw {
    pub fn packet(self, pass: RenderPassKind) -> DrawPacket {
        DrawPacket {
            pass,
            pipeline: self.pipeline,
            bind_group: self.bind_group,
            vertex_buffer: self.vertex_buffer,
            index_buffer: self.index_buffer,
            material: self.material,
            push_constants: self.push_constants,
            index_count: self.index_count,
            first_index: self.first_index,
            vertex_offset: self.vertex_offset,
            sort_key: self.sort_key,
        }
    }
}

pub(crate) fn submit_draws<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pass: RenderPassKind,
    draws: &[MeshDraw],
) {
    let queue = runtime.queue_mut();
    for draw in draws {
        queue.push((*draw).packet(pass));
    }
}
