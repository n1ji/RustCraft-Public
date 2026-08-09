use renderer_api::core::{MaterialDesc, RenderPassKind, RendererRuntime};
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, Extent2D, MaterialHandle, PipelineHandle, RhiResult, SamplerDesc, SamplerHandle,
    TextureDesc, TextureFormat, TextureHandle, TextureUsage,
};

use crate::render::gui::GuiVertexBuilder;
use crate::ui::font::FontRenderer;

use crate::render::gui::ButtonHit;
use crate::{client::player::Camera, render::scene::EntityBillboard};

use super::{submit_draws, MeshDraw};

pub const GUI_TEXTURE_COUNT: usize = 11;
const GUI_BATCH_COUNT: usize = GUI_TEXTURE_COUNT + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum GuiTextureKind {
    Font = 0,
    Widgets = 1,
    Blocks = 2,
    Inventory = 3,
    Generic54 = 4,
    Items = 5,
    Icons = 6,
    Creative = 7,
    OptionsBackground = 8,
    Underwater = 9,
    Overlay = 10,
}

impl GuiTextureKind {
    pub const ALL: [Self; GUI_TEXTURE_COUNT] = [
        Self::Font,
        Self::Widgets,
        Self::Blocks,
        Self::Inventory,
        Self::Generic54,
        Self::Items,
        Self::Icons,
        Self::Creative,
        Self::OptionsBackground,
        Self::Underwater,
        Self::Overlay,
    ];

    const fn index(self) -> usize {
        self as usize
    }
}

pub const CREATIVE_TAB_BLOCK: usize = 0;
pub const CREATIVE_TAB_DECORATION: usize = 1;
pub const CREATIVE_TAB_REDSTONE: usize = 2;
pub const CREATIVE_TAB_TRANSPORT: usize = 3;
pub const CREATIVE_TAB_MISC: usize = 4;
pub const CREATIVE_TAB_SEARCH: usize = 5;
pub const CREATIVE_TAB_FOOD: usize = 6;
pub const CREATIVE_TAB_TOOLS: usize = 7;
pub const CREATIVE_TAB_COMBAT: usize = 8;
pub const CREATIVE_TAB_BREWING: usize = 9;
pub const CREATIVE_TAB_MATERIALS: usize = 10;
pub const CREATIVE_TAB_INVENTORY: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CreativeItemEntry {
    pub item_id: u16,
    pub damage: u16,
}

/// The vanilla 1.8.9 creative-tab order. Item rendering stays in the game
/// crate; this data is only the inventory model consumed by the GUI system.
pub fn creative_tab_items(tab: usize) -> Vec<u16> {
    match tab {
        CREATIVE_TAB_BLOCK => vec![
            1, 2, 3, 4, 5, 7, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 24, 35, 41, 42, 44, 45, 47,
            48, 49, 53, 56, 57, 67, 73, 79, 80, 82, 86, 87, 88, 89, 91, 95, 98, 103, 108, 109, 110,
            112, 114, 121, 126, 128, 129, 133, 134, 135, 136, 139, 153, 155, 156, 159, 162, 163,
            164, 168, 169, 170, 172, 173, 174, 179, 180, 182,
        ],
        CREATIVE_TAB_DECORATION => vec![
            6, 18, 30, 31, 32, 37, 38, 39, 40, 50, 54, 58, 61, 65, 78, 81, 84, 85, 97, 101, 102,
            106, 111, 113, 116, 120, 130, 145, 146, 160, 161, 165, 171, 175, 188, 189, 190, 191,
            192, 321, 323, 355, 389, 390, 397, 416, 425,
        ],
        CREATIVE_TAB_REDSTONE => vec![
            23, 25, 29, 33, 46, 69, 70, 72, 76, 77, 96, 107, 123, 131, 143, 147, 148, 151, 152,
            154, 158, 167, 183, 184, 185, 186, 187, 324, 330, 331, 356, 404, 427, 428, 429, 430,
            431,
        ],
        CREATIVE_TAB_TRANSPORT => vec![27, 28, 66, 157, 328, 329, 333, 342, 343, 398, 407, 408],
        CREATIVE_TAB_MISC => vec![
            138, 325, 326, 327, 332, 335, 339, 340, 341, 352, 368, 381, 383, 384, 385, 386, 395,
            402, 417, 418, 419, 2256, 2257, 2258, 2259, 2260, 2261, 2262, 2263, 2264, 2265, 2266,
            2267,
        ],
        CREATIVE_TAB_SEARCH => Vec::new(),
        CREATIVE_TAB_FOOD => vec![
            260, 282, 297, 319, 320, 322, 349, 350, 354, 357, 360, 363, 364, 365, 366, 367, 375,
            391, 392, 393, 394, 400, 411, 412, 413, 423, 424,
        ],
        CREATIVE_TAB_TOOLS => vec![
            256, 257, 258, 259, 269, 270, 271, 273, 274, 275, 277, 278, 279, 284, 285, 286, 290,
            291, 292, 293, 294, 345, 346, 347, 359, 420, 421,
        ],
        CREATIVE_TAB_COMBAT => vec![
            261, 262, 267, 268, 272, 276, 283, 298, 299, 300, 301, 302, 303, 304, 305, 306, 307,
            308, 309, 310, 311, 312, 313, 314, 315, 316, 317,
        ],
        CREATIVE_TAB_BREWING => vec![370, 373, 374, 376, 377, 378, 379, 380, 382, 396, 414],
        CREATIVE_TAB_MATERIALS => vec![
            263, 264, 265, 266, 280, 281, 287, 288, 289, 295, 296, 318, 334, 336, 337, 338, 344,
            348, 351, 353, 361, 362, 369, 371, 372, 388, 399, 405, 406, 409, 410, 415,
        ],
        CREATIVE_TAB_INVENTORY => Vec::new(),
        _ => Vec::new(),
    }
}

pub fn creative_tab_entries(tab: usize) -> Vec<CreativeItemEntry> {
    if tab == CREATIVE_TAB_SEARCH {
        let mut seen = std::collections::HashSet::new();
        return [
            CREATIVE_TAB_BLOCK,
            CREATIVE_TAB_DECORATION,
            CREATIVE_TAB_REDSTONE,
            CREATIVE_TAB_TRANSPORT,
            CREATIVE_TAB_MISC,
            CREATIVE_TAB_FOOD,
            CREATIVE_TAB_TOOLS,
            CREATIVE_TAB_COMBAT,
            CREATIVE_TAB_BREWING,
            CREATIVE_TAB_MATERIALS,
        ]
        .into_iter()
        .flat_map(creative_tab_entries)
        .filter(|entry| seen.insert(*entry))
        .collect();
    }

    let mut entries = creative_tab_items(tab)
        .into_iter()
        .map(|item_id| CreativeItemEntry { item_id, damage: 0 })
        .collect::<Vec<_>>();
    let mut add_variants = |item_id: u16, damages: &[u16]| {
        for &damage in damages {
            let entry = CreativeItemEntry { item_id, damage };
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
    };
    match tab {
        CREATIVE_TAB_BLOCK => {
            add_variants(1, &[1, 2, 3, 4, 5, 6]);
            add_variants(3, &[1, 2]);
            add_variants(5, &[1, 2, 3, 4, 5]);
            add_variants(35, &(1..=15).collect::<Vec<_>>());
            add_variants(44, &[1, 2, 3, 4, 5, 6, 7]);
            add_variants(95, &(1..=15).collect::<Vec<_>>());
            add_variants(159, &(1..=15).collect::<Vec<_>>());
        }
        CREATIVE_TAB_DECORATION => {
            add_variants(6, &[1, 2, 3, 4, 5]);
            add_variants(18, &[1, 2, 3]);
            add_variants(38, &[1, 2, 3, 4, 5, 6, 7, 8]);
            add_variants(160, &(1..=15).collect::<Vec<_>>());
            add_variants(171, &(1..=15).collect::<Vec<_>>());
            add_variants(397, &[1, 2, 3, 4]);
        }
        CREATIVE_TAB_MISC => add_variants(
            383,
            &[
                50, 51, 52, 54, 55, 56, 57, 58, 59, 60, 61, 62, 65, 66, 67, 68, 90, 91, 92, 93, 94,
                95, 96, 98, 100, 101, 120,
            ],
        ),
        CREATIVE_TAB_FOOD => {
            add_variants(322, &[1]);
            add_variants(349, &[1, 2, 3]);
            add_variants(350, &[1]);
        }
        CREATIVE_TAB_BREWING => add_variants(
            373,
            &[
                16, 32, 64, 8193, 8194, 8195, 8196, 8197, 8198, 8200, 8201, 8202, 8204, 8205, 8206,
                8225, 8226, 8228, 8229, 8233, 8234, 16385, 16386, 16387, 16388, 16389, 16390,
                16392, 16393, 16394, 16396, 16397, 16398,
            ],
        ),
        CREATIVE_TAB_MATERIALS => {
            add_variants(263, &[1]);
            add_variants(351, &(1..=15).collect::<Vec<_>>());
        }
        _ => {}
    }
    entries
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreativeScrollbarGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub thumb_height: f32,
    pub scale: f32,
}

impl CreativeScrollbarGeometry {
    pub fn contains(self, mouse_x: f32, mouse_y: f32) -> bool {
        mouse_x >= self.x
            && mouse_x < self.x + self.width
            && mouse_y >= self.y
            && mouse_y < self.y + self.height
    }

    pub fn scroll_for_mouse_y(self, mouse_y: f32) -> f32 {
        ((mouse_y - self.y - 7.5 * self.scale) / (self.height - self.thumb_height)).clamp(0.0, 1.0)
    }
}

pub fn creative_scrollbar_geometry(
    viewport_width: f32,
    viewport_height: f32,
    gui_scale: f32,
) -> CreativeScrollbarGeometry {
    let scale = gui_scale.max(1.0);
    let panel_x = (viewport_width - 195.0 * scale) * 0.5;
    let panel_y = (viewport_height - 136.0 * scale) * 0.5;
    CreativeScrollbarGeometry {
        x: panel_x + 175.0 * scale,
        y: panel_y + 18.0 * scale,
        width: 14.0 * scale,
        height: 112.0 * scale,
        thumb_height: 15.0 * scale,
        scale,
    }
}

pub fn creative_max_scroll_rows(item_count: usize) -> usize {
    item_count.div_ceil(9).saturating_sub(5)
}

pub struct GuiRenderer {
    draws: Vec<MeshDraw>,
    pub font: FontRenderer,
    /// World-space entity names are projected once per frame into this
    /// dedicated font batch.  Keeping them separate from the HUD preserves
    /// the HUD's cached geometry while still using the new RHI submission
    /// path.
    pub nametag_batch: GuiVertexBuilder,
    pub overlay_batch: GuiVertexBuilder,
    pub background_batch: GuiVertexBuilder,
    pub widget_batch: GuiVertexBuilder,
    pub font_batch: GuiVertexBuilder,
    pub inventory_batch: GuiVertexBuilder,
    pub generic54_batch: GuiVertexBuilder,
    pub block_batch: GuiVertexBuilder,
    pub item_batch: GuiVertexBuilder,
    pub icons_batch: GuiVertexBuilder,
    pub creative_batch: GuiVertexBuilder,
    pub underwater_batch: GuiVertexBuilder,
    pub button_hits: Vec<ButtonHit>,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub creative_tab: usize,
    pub creative_scroll: f32,
    pub creative_search: String,
    pub viewport: [u32; 2],
    pub generation: u64,
    pub dirty: bool,
    nametag_hash: u64,
    /// A camera-only name-tag update must not force every static HUD batch
    /// through the hash/upload path. This flag is set only when the other
    /// batches are already resident on the GPU.
    nametag_only_dirty: bool,
    textures: [Option<TextureHandle>; GUI_TEXTURE_COUNT],
    owned_textures: [bool; GUI_TEXTURE_COUNT],
    gpu: Option<GuiGpuState>,
}

struct GuiGpuState {
    pipeline: PipelineHandle,
    uniform_buffer: BufferHandle,
    sampler: SamplerHandle,
    fallback_texture: TextureHandle,
    bindings: Vec<GuiTextureBinding>,
    batch_meshes: Vec<GuiBatchGpu>,
    uploaded_generation: u64,
    uploaded_viewport: [u32; 2],
}

#[derive(Default)]
struct GuiBatchGpu {
    draw: Option<MeshDraw>,
    vertex_hash: u64,
    index_hash: u64,
    vertex_capacity: u64,
    index_capacity: u64,
}

struct GuiTextureBinding {
    texture: TextureHandle,
    bind_group: BindGroupHandle,
    material: MaterialHandle,
}

impl Default for GuiRenderer {
    fn default() -> Self {
        Self {
            draws: Vec::new(),
            font: FontRenderer::new(),
            nametag_batch: GuiVertexBuilder::new(),
            overlay_batch: GuiVertexBuilder::new(),
            background_batch: GuiVertexBuilder::new(),
            widget_batch: GuiVertexBuilder::new(),
            font_batch: GuiVertexBuilder::new(),
            inventory_batch: GuiVertexBuilder::new(),
            generic54_batch: GuiVertexBuilder::new(),
            block_batch: GuiVertexBuilder::new(),
            item_batch: GuiVertexBuilder::new(),
            icons_batch: GuiVertexBuilder::new(),
            creative_batch: GuiVertexBuilder::new(),
            underwater_batch: GuiVertexBuilder::new(),
            button_hits: Vec::new(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            creative_tab: CREATIVE_TAB_BLOCK,
            creative_scroll: 0.0,
            creative_search: String::new(),
            viewport: [0, 0],
            generation: 0,
            dirty: true,
            nametag_hash: 0,
            nametag_only_dirty: false,
            textures: [None; GUI_TEXTURE_COUNT],
            owned_textures: [false; GUI_TEXTURE_COUNT],
            gpu: None,
        }
    }
}

impl GuiRenderer {
    pub fn clear_batches(&mut self) {
        self.nametag_batch.clear();
        self.overlay_batch.clear();
        self.background_batch.clear();
        self.widget_batch.clear();
        self.font_batch.clear();
        self.inventory_batch.clear();
        self.generic54_batch.clear();
        self.block_batch.clear();
        self.item_batch.clear();
        self.icons_batch.clear();
        self.creative_batch.clear();
        self.underwater_batch.clear();
        self.button_hits.clear();
        self.generation = self.generation.wrapping_add(1);
        self.nametag_hash = 0;
        self.nametag_only_dirty = false;
        self.dirty = true;
    }

    pub fn refresh_button_hits(&mut self) {
        self.button_hits.clear();
        for batch in [
            &self.overlay_batch,
            &self.background_batch,
            &self.widget_batch,
            &self.font_batch,
            &self.inventory_batch,
            &self.generic54_batch,
            &self.block_batch,
            &self.item_batch,
            &self.icons_batch,
            &self.creative_batch,
            &self.underwater_batch,
        ] {
            self.button_hits.extend(batch.button_hits.iter().copied());
        }
    }

    /// Rebuild the projected player/entity name batch using only game-side
    /// camera data and the backend-neutral GUI vertex builder.  The batch is
    /// cached at sub-pixel precision so a static camera does not cause a GPU
    /// upload every redraw.
    pub fn update_nametags(
        &mut self,
        camera: &Camera,
        billboards: &[EntityBillboard],
        viewport: [u32; 2],
        enabled: bool,
    ) {
        use std::hash::{Hash, Hasher};

        let width = viewport[0].max(1) as f32;
        let height = viewport[1].max(1) as f32;
        let view_proj = camera.projection_matrix_at(camera.partial_tick) * camera.view_matrix();
        let focal_y = camera.projection_matrix_at(camera.partial_tick)[(1, 1)].abs() * height * 0.5;
        let mut hasher = fnv::FnvHasher::default();
        hasher.write_u8(enabled as u8);
        hasher.write_u32(viewport[0]);
        hasher.write_u32(viewport[1]);

        let mut projected = Vec::new();
        if enabled {
            for billboard in billboards {
                let Some(name) = billboard.name.as_deref().filter(|name| !name.is_empty()) else {
                    continue;
                };
                if !billboard.name_visible || billboard.invisible {
                    continue;
                }

                let dx = billboard.position[0] - camera.position.x;
                let dy = billboard.position[1] + billboard.height - camera.position.y;
                let dz = billboard.position[2] - camera.position.z;
                let distance_sq = dx * dx + dy * dy + dz * dz;
                if distance_sq > 64.0 * 64.0 {
                    continue;
                }

                let clip = view_proj
                    * nalgebra::Vector4::new(
                        billboard.position[0],
                        billboard.position[1] + billboard.height + 0.5,
                        billboard.position[2],
                        1.0,
                    );
                if clip.w <= 0.001 {
                    continue;
                }
                let ndc = clip.xyz() / clip.w;
                if ndc.z < -0.05
                    || ndc.z > 1.05
                    || ndc.x < -1.25
                    || ndc.x > 1.25
                    || ndc.y < -1.25
                    || ndc.y > 1.25
                {
                    continue;
                }

                let screen_x = (ndc.x * 0.5 + 0.5) * width;
                let screen_y = (ndc.y * 0.5 + 0.5) * height;
                // Vanilla's name plate is a 0.026666 world-unit scale.  A
                // small lower bound keeps distant labels legible with the
                // bitmap/font atlas while preserving perspective shrinkage.
                let size = (focal_y * 0.22 / clip.w).clamp(8.0, 32.0);
                let quantize = |value: f32| (value * 4.0).round() as i32;
                name.hash(&mut hasher);
                hasher.write_i32(quantize(screen_x));
                hasher.write_i32(quantize(screen_y));
                hasher.write_i32(quantize(size));
                projected.push((screen_x, screen_y, size, name.to_owned()));
            }
        }

        let hash = hasher.finish();
        if self.nametag_hash == hash {
            return;
        }
        self.nametag_hash = hash;
        // `dirty` is set by the normal GUI builder for full HUD/screen
        // rebuilds. Only a clean renderer can take the cheap single-batch
        // path below.
        self.nametag_only_dirty = !self.dirty;
        self.nametag_batch.clear();
        for (screen_x, screen_y, size, name) in projected {
            let text_width = self.font.text_width(&name, size);
            let top = screen_y - size - 2.0;
            let box_width = text_width + 6.0;
            let box_height = size + 3.0;
            self.nametag_batch.fill_rect(
                screen_x - box_width * 0.5,
                top - 1.0,
                box_width,
                box_height,
                [0.0, 0.0, 0.0, 0.38],
            );
            self.nametag_batch.draw_text_centered(
                &mut self.font,
                screen_x + 1.0,
                top + 1.0,
                &name,
                size,
                [0.0, 0.0, 0.0, 0.9],
            );
            self.nametag_batch.draw_text_centered(
                &mut self.font,
                screen_x,
                top,
                &name,
                size,
                [1.0, 1.0, 1.0, 1.0],
            );
        }
        self.generation = self.generation.wrapping_add(1);
        self.dirty = true;
    }
}

fn gui_buffer_hash(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;

    let mut hasher = fnv::FnvHasher::default();
    hasher.write_usize(bytes.len());
    hasher.write(bytes);
    hasher.finish()
}

fn create_gui_buffer<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    bytes: &[u8],
    usage: BufferUsage,
    label: &str,
) -> RhiResult<BufferHandle> {
    let handle =
        runtime.create_buffer(BufferDesc::new(bytes.len() as u64, usage, label).device_local())?;
    if let Err(error) = runtime.device().upload_buffer(handle, 0, bytes) {
        let _ = runtime.destroy_buffer(handle);
        return Err(error);
    }
    Ok(handle)
}

impl GuiRenderer {
    pub fn set_draws(&mut self, draws: Vec<MeshDraw>) {
        self.draws = draws;
    }

    /// Create the GUI's RHI-owned uniform, sampler, fallback texture, bind
    /// group and material. The shader/pipeline itself is supplied by the
    /// application composition root, so this remains backend-neutral.
    pub fn configure_gpu<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
    ) -> RhiResult<()> {
        self.destroy_gpu(runtime);

        self.font.preload_ascii();

        let uniform_size = std::mem::size_of::<crate::render::gui::GuiUniforms>() as u64;
        let uniform_buffer = runtime.create_buffer(
            BufferDesc::new(
                uniform_size,
                BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                "game.gui.uniforms",
            )
            .device_local(),
        )?;
        if let Err(error) = self.upload_uniform(runtime, uniform_buffer, [0, 0]) {
            let _ = runtime.destroy_buffer(uniform_buffer);
            return Err(error);
        }

        let sampler = match runtime.create_sampler(SamplerDesc {
            // The vanilla GUI textures are pixel art and their full-screen
            // backgrounds/underwater overlay intentionally tile outside the
            // normalized 0..1 range.
            linear_filter: false,
            repeat_u: true,
            repeat_v: true,
            repeat_w: false,
            label: String::from("game.gui.sampler"),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };

        let fallback_texture = match Self::upload_texture(
            runtime,
            1,
            1,
            &[255, 255, 255, 255],
            "game.gui.fallback-texture",
        ) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_sampler(sampler);
                let _ = runtime.destroy_buffer(uniform_buffer);
                return Err(error);
            }
        };

        if self.textures[GuiTextureKind::Font.index()].is_none() {
            match Self::upload_texture(
                runtime,
                self.font.atlas_width,
                self.font.atlas_height,
                &self.font.atlas_pixels,
                "game.gui.font-atlas",
            ) {
                Ok(texture) => {
                    self.set_texture(GuiTextureKind::Font, Some(texture), true);
                    self.font.atlas_dirty = false;
                }
                Err(error) => {
                    let _ = runtime.destroy_texture(fallback_texture);
                    let _ = runtime.destroy_sampler(sampler);
                    let _ = runtime.destroy_buffer(uniform_buffer);
                    return Err(error);
                }
            }
        }

        let mut bindings = Vec::with_capacity(GUI_TEXTURE_COUNT);
        for kind in GuiTextureKind::ALL {
            let texture = self
                .valid_texture(runtime, kind)
                .unwrap_or(fallback_texture);
            match Self::create_binding(runtime, pipeline, uniform_buffer, texture, sampler) {
                Ok(binding) => bindings.push(binding),
                Err(error) => {
                    for binding in bindings.drain(..) {
                        let _ = runtime.destroy_material(binding.material);
                        let _ = runtime.destroy_bind_group(binding.bind_group);
                    }
                    let _ = runtime.destroy_texture(fallback_texture);
                    let _ = runtime.destroy_sampler(sampler);
                    let _ = runtime.destroy_buffer(uniform_buffer);
                    return Err(error);
                }
            }
        }

        self.gpu = Some(GuiGpuState {
            pipeline,
            uniform_buffer,
            sampler,
            fallback_texture,
            bindings,
            batch_meshes: (0..GUI_BATCH_COUNT)
                .map(|_| GuiBatchGpu::default())
                .collect(),
            uploaded_generation: u64::MAX,
            uploaded_viewport: [u32::MAX, u32::MAX],
        });
        Ok(())
    }

    /// Upload changed GUI batches once and turn them into RHI draw packets.
    /// Rebuilding menus remains a CPU operation; GPU buffers are replaced only
    /// when an individual batch's contents grow or change.
    pub fn sync_gpu<D: RenderDevice>(&mut self, runtime: &mut RendererRuntime<D>) -> RhiResult<()> {
        let Some(mut gpu) = self.gpu.take() else {
            return Ok(());
        };

        let result = self.sync_gpu_state(runtime, &mut gpu);
        self.gpu = Some(gpu);
        result
    }

    pub fn destroy_gpu<D: RenderDevice>(&mut self, runtime: &mut RendererRuntime<D>) {
        let Some(gpu) = self.gpu.take() else {
            for draw in self.draws.drain(..) {
                let _ = super::mesh::destroy_mesh(runtime, draw);
            }
            return;
        };
        for batch in &gpu.batch_meshes {
            if let Some(draw) = batch.draw {
                self.draws.retain(|current| {
                    current.vertex_buffer != draw.vertex_buffer
                        || current.index_buffer != draw.index_buffer
                });
                let _ = super::mesh::destroy_mesh(runtime, draw);
            }
        }
        for draw in self.draws.drain(..) {
            let _ = super::mesh::destroy_mesh(runtime, draw);
        }
        for binding in gpu.bindings {
            let _ = runtime.destroy_material(binding.material);
            let _ = runtime.destroy_bind_group(binding.bind_group);
        }
        let _ = runtime.destroy_buffer(gpu.uniform_buffer);
        let _ = runtime.destroy_sampler(gpu.sampler);
        let _ = runtime.destroy_texture(gpu.fallback_texture);
    }

    pub fn pipeline(&self) -> Option<PipelineHandle> {
        self.gpu.as_ref().map(|gpu| gpu.pipeline)
    }

    pub fn texture(&self, kind: GuiTextureKind) -> Option<TextureHandle> {
        self.textures[kind.index()]
    }

    pub fn set_texture(
        &mut self,
        kind: GuiTextureKind,
        texture: Option<TextureHandle>,
        owned: bool,
    ) {
        self.textures[kind.index()] = texture;
        self.owned_textures[kind.index()] = texture.is_some() && owned;
        self.invalidate();
    }

    pub fn destroy_owned_textures<D: RenderDevice>(&mut self, runtime: &mut RendererRuntime<D>) {
        debug_assert!(
            self.gpu.is_none(),
            "destroy GUI bindings before GUI textures"
        );
        for kind in GuiTextureKind::ALL {
            let index = kind.index();
            if self.owned_textures[index] {
                if let Some(texture) = self.textures[index].take() {
                    let _ = runtime.destroy_texture(texture);
                }
            } else {
                self.textures[index] = None;
            }
            self.owned_textures[index] = false;
        }
    }

    pub fn upload_batch<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        bind_group: Option<BindGroupHandle>,
        batch: &GuiVertexBuilder,
        label: &str,
    ) -> RhiResult<()> {
        if let Some(draw) =
            super::mesh::upload_gui_batch(runtime, pipeline, bind_group, batch, label)?
        {
            self.draws.push(draw);
        }
        Ok(())
    }

    pub fn draws(&self) -> &[MeshDraw] {
        &self.draws
    }

    pub fn hit_test(&self, mouse_x: f32, mouse_y: f32) -> Option<u32> {
        self.button_hits.iter().rev().find_map(|hit| {
            (mouse_x >= hit.x
                && mouse_x <= hit.x + hit.w
                && mouse_y >= hit.y
                && mouse_y <= hit.y + hit.h)
                .then_some(hit.id)
        })
    }

    pub fn hit_rect(&self, id: u32) -> Option<&ButtonHit> {
        self.button_hits.iter().rev().find(|hit| hit.id == id)
    }

    pub fn directional_hit(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        direction: [f32; 2],
    ) -> Option<ButtonHit> {
        let [dx, dy] = direction;
        self.button_hits
            .iter()
            .copied()
            .filter_map(|hit| {
                let center_x = hit.x + hit.w * 0.5;
                let center_y = hit.y + hit.h * 0.5;
                let offset_x = center_x - mouse_x;
                let offset_y = center_y - mouse_y;
                let forward = offset_x * dx + offset_y * dy;
                (forward > 0.5)
                    .then_some((forward + (offset_x * dy - offset_y * dx).abs() * 0.35, hit))
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, hit)| hit)
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.nametag_only_dirty = false;
        self.dirty = true;
    }

    pub fn submit<D: RenderDevice>(&self, runtime: &mut RendererRuntime<D>) {
        submit_draws(runtime, RenderPassKind::Gui, &self.draws);
    }

    fn valid_texture<D: RenderDevice>(
        &self,
        runtime: &RendererRuntime<D>,
        kind: GuiTextureKind,
    ) -> Option<TextureHandle> {
        self.textures[kind.index()].filter(|handle| runtime.resources().texture(*handle).is_some())
    }

    pub fn upload_texture<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        width: u32,
        height: u32,
        pixels: &[u8],
        label: &str,
    ) -> RhiResult<TextureHandle> {
        let width = width.max(1);
        let height = height.max(1);
        let mut desc = TextureDesc::new(
            Extent2D::new(width, height),
            TextureFormat::R8G8B8A8Srgb,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        );
        desc.label = label.to_string();
        let texture = runtime.create_texture(desc)?;
        if let Err(error) =
            runtime
                .device()
                .upload_texture(texture, pixels, width.saturating_mul(4))
        {
            let _ = runtime.destroy_texture(texture);
            return Err(error);
        }
        Ok(texture)
    }

    fn create_bind_group<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: BufferHandle,
        texture: TextureHandle,
        sampler: SamplerHandle,
    ) -> RhiResult<BindGroupHandle> {
        runtime.create_bind_group(BindGroupDesc::new(
            pipeline,
            0,
            vec![
                BindGroupEntry::new(
                    0,
                    BindingResource::Buffer {
                        buffer: uniform_buffer,
                        offset: 0,
                        size: std::mem::size_of::<crate::render::gui::GuiUniforms>() as u64,
                    },
                ),
                BindGroupEntry::new(
                    1,
                    BindingResource::CombinedImageSampler { texture, sampler },
                ),
            ],
        ))
    }

    fn create_binding<D: RenderDevice>(
        runtime: &mut RendererRuntime<D>,
        pipeline: PipelineHandle,
        uniform_buffer: BufferHandle,
        texture: TextureHandle,
        sampler: SamplerHandle,
    ) -> RhiResult<GuiTextureBinding> {
        let bind_group =
            Self::create_bind_group(runtime, pipeline, uniform_buffer, texture, sampler)?;
        let material = match runtime.create_material(MaterialDesc {
            pipeline,
            bind_groups: vec![bind_group],
            textures: vec![texture],
            uniform_buffer: Some(uniform_buffer),
            push_constants: Vec::new(),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_bind_group(bind_group);
                return Err(error);
            }
        };
        Ok(GuiTextureBinding {
            texture,
            bind_group,
            material,
        })
    }

    fn upload_uniform<D: RenderDevice>(
        &self,
        runtime: &mut RendererRuntime<D>,
        buffer: BufferHandle,
        viewport: [u32; 2],
    ) -> RhiResult<()> {
        let uniforms = crate::render::gui::GuiUniforms::new(
            viewport[0].max(1) as f32,
            viewport[1].max(1) as f32,
        );
        runtime
            .device()
            .upload_buffer(buffer, 0, bytemuck::bytes_of(&uniforms))
    }

    fn sync_gpu_state<D: RenderDevice>(
        &mut self,
        runtime: &mut RendererRuntime<D>,
        gpu: &mut GuiGpuState,
    ) -> RhiResult<()> {
        let viewport = [self.viewport[0].max(1), self.viewport[1].max(1)];
        if gpu.uploaded_viewport != viewport {
            self.upload_uniform(runtime, gpu.uniform_buffer, viewport)?;
            gpu.uploaded_viewport = viewport;
        }

        if self.font.atlas_dirty {
            if let Some(texture) = self.valid_texture(runtime, GuiTextureKind::Font) {
                runtime.device().upload_texture(
                    texture,
                    &self.font.atlas_pixels,
                    self.font.atlas_width.saturating_mul(4),
                )?;
                self.font.atlas_dirty = false;
            }
        }

        let mut bindings_changed = false;
        for kind in GuiTextureKind::ALL {
            let index = kind.index();
            let desired = self
                .valid_texture(runtime, kind)
                .unwrap_or(gpu.fallback_texture);
            if gpu.bindings[index].texture == desired {
                continue;
            }
            let replacement = Self::create_binding(
                runtime,
                gpu.pipeline,
                gpu.uniform_buffer,
                desired,
                gpu.sampler,
            )?;
            let old = std::mem::replace(&mut gpu.bindings[index], replacement);
            let _ = runtime.destroy_material(old.material);
            let _ = runtime.destroy_bind_group(old.bind_group);
            bindings_changed = true;
        }
        if bindings_changed {
            self.nametag_only_dirty = false;
            self.dirty = true;
        }

        // Most redraws reuse the exact same GUI buffers.  Avoid walking and
        // hashing every vertex/index byte unless the CPU-side batches or a
        // texture binding actually changed.
        if !self.dirty
            && !self.font.atlas_dirty
            && !bindings_changed
            && gpu.uploaded_generation == self.generation
        {
            return Ok(());
        }

        let batches = [
            (
                GuiTextureKind::OptionsBackground,
                &self.background_batch,
                "background",
            ),
            (GuiTextureKind::Font, &self.nametag_batch, "nametags"),
            (GuiTextureKind::Overlay, &self.overlay_batch, "overlay"),
            (GuiTextureKind::Widgets, &self.widget_batch, "widgets"),
            (
                GuiTextureKind::Inventory,
                &self.inventory_batch,
                "inventory",
            ),
            (
                GuiTextureKind::Generic54,
                &self.generic54_batch,
                "generic54",
            ),
            (GuiTextureKind::Blocks, &self.block_batch, "blocks"),
            (GuiTextureKind::Items, &self.item_batch, "items"),
            (GuiTextureKind::Icons, &self.icons_batch, "icons"),
            (GuiTextureKind::Creative, &self.creative_batch, "creative"),
            (GuiTextureKind::Font, &self.font_batch, "font"),
            // Vanilla draws the underwater texture after the complete HUD,
            // so it must remain the final GUI batch.
            (
                GuiTextureKind::Underwater,
                &self.underwater_batch,
                "underwater",
            ),
        ];
        self.draws.clear();
        let nametag_only_dirty =
            self.nametag_only_dirty && !bindings_changed && !self.font.atlas_dirty;
        for (batch_index, (kind, batch, label)) in batches.into_iter().enumerate() {
            let binding = &gpu.bindings[kind.index()];
            let cache = &mut gpu.batch_meshes[batch_index];
            if nametag_only_dirty && batch_index != 1 {
                if let Some(draw) = cache.draw {
                    self.draws.push(draw);
                }
                continue;
            }
            if batch.vertices.is_empty() || batch.indices.is_empty() {
                if let Some(draw) = cache.draw.take() {
                    let _ = super::mesh::destroy_mesh(runtime, draw);
                }
                cache.vertex_hash = 0;
                cache.index_hash = 0;
                cache.vertex_capacity = 0;
                cache.index_capacity = 0;
                continue;
            }

            let vertex_bytes = bytemuck::cast_slice::<_, u8>(&batch.vertices);
            let index_bytes = bytemuck::cast_slice::<_, u8>(&batch.indices);
            let vertex_hash = gui_buffer_hash(vertex_bytes);
            let index_hash = gui_buffer_hash(index_bytes);

            if cache.draw.is_none() {
                let Some(mut draw) = super::mesh::upload_gui_batch(
                    runtime,
                    gpu.pipeline,
                    Some(binding.bind_group),
                    batch,
                    &format!("game.gui.{label}"),
                )?
                else {
                    continue;
                };
                draw.material = Some(binding.material);
                draw.sort_key = self.draws.len() as u64;
                cache.vertex_capacity = vertex_bytes.len() as u64;
                cache.index_capacity = index_bytes.len() as u64;
                cache.vertex_hash = vertex_hash;
                cache.index_hash = index_hash;
                cache.draw = Some(draw);
            } else {
                let draw = cache.draw.as_mut().expect("GUI batch draw exists");
                if cache.vertex_capacity < vertex_bytes.len() as u64 {
                    let replacement = create_gui_buffer(
                        runtime,
                        vertex_bytes,
                        BufferUsage::VERTEX | BufferUsage::TRANSFER_DST,
                        &format!("game.gui.{label}.vertices"),
                    )?;
                    let old = std::mem::replace(&mut draw.vertex_buffer, replacement);
                    let _ = runtime.destroy_buffer(old);
                    cache.vertex_capacity = vertex_bytes.len() as u64;
                } else if cache.vertex_hash != vertex_hash {
                    runtime
                        .device()
                        .upload_buffer(draw.vertex_buffer, 0, vertex_bytes)?;
                }

                if cache.index_capacity < index_bytes.len() as u64 {
                    let replacement = create_gui_buffer(
                        runtime,
                        index_bytes,
                        BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
                        &format!("game.gui.{label}.indices"),
                    )?;
                    if let Some(old) = draw.index_buffer.replace(replacement) {
                        let _ = runtime.destroy_buffer(old);
                    }
                    cache.index_capacity = index_bytes.len() as u64;
                } else if cache.index_hash != index_hash {
                    if let Some(index_buffer) = draw.index_buffer {
                        runtime
                            .device()
                            .upload_buffer(index_buffer, 0, index_bytes)?;
                    }
                }

                draw.pipeline = gpu.pipeline;
                draw.bind_group = Some(binding.bind_group);
                draw.material = Some(binding.material);
                draw.index_count = batch.indices.len() as u32;
                draw.sort_key = self.draws.len() as u64;
                cache.vertex_hash = vertex_hash;
                cache.index_hash = index_hash;
            }

            if let Some(draw) = cache.draw {
                self.draws.push(draw);
            }
        }
        gpu.uploaded_generation = self.generation;
        self.nametag_only_dirty = false;
        self.dirty = false;
        Ok(())
    }
}
