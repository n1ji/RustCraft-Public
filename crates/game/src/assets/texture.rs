//! Dynamic texture atlas for block, item, particle, and special block textures.
//!
//! Textures keep their resource-pack resolution. Rectangles are packed at runtime
//! and each atlas index publishes its own UV region, so a 128x128 texture is never
//! reduced to Minecraft's vanilla 16x16 resolution.

use image::{DynamicImage, GenericImageView, RgbaImage};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};

use super::resolver::AssetResolver;

pub const TILE_SIZE: u32 = 16;
pub const ATLAS_TILES_X: u32 = 64;
pub const ATLAS_TILES_Y: u32 = 64;
pub const ATLAS_TILES: u32 = ATLAS_TILES_X;
pub const ATLAS_WIDTH: u32 = TILE_SIZE * ATLAS_TILES_X;
pub const ATLAS_HEIGHT: u32 = TILE_SIZE * ATLAS_TILES_Y;
pub const MAX_TILES: usize = (ATLAS_TILES_X * ATLAS_TILES_Y) as usize;
pub const MISSING_TILE_INDEX: usize = MAX_TILES - 2;
pub const WHITE_TILE_INDEX: usize = MAX_TILES - 1;

const CONTENT_TILE_LIMIT: usize = MISSING_TILE_INDEX;
const ATLAS_PADDING: u32 = 1;
const MAX_ATLAS_DIMENSION: u32 = 16_384;

/// Cap texture dimensions to prevent a single resource-pack texture from
/// ballooning the atlas. 256 px covers standard high-res packs; anything
/// larger provides negligible visual gain in a block game.
const MAX_TEX_DIM: u32 = 256;
const UV_COMPONENT_COUNT: usize = MAX_TILES * 4;

static TEXTURE_MAP: std::sync::Mutex<Option<HashMap<String, usize>>> = std::sync::Mutex::new(None);
static ATLAS_UV_BITS: [AtomicU32; UV_COMPONENT_COUNT] =
    [const { AtomicU32::new(0) }; UV_COMPONENT_COUNT];

/// Publish texture names and packed UVs for mesh and UI code.
pub fn init_texture_map(atlas: &TextureAtlas) {
    if let Ok(mut map) = TEXTURE_MAP.lock() {
        *map = Some(atlas.name_to_index.clone());
    }

    let missing = atlas
        .regions
        .get(MISSING_TILE_INDEX)
        .copied()
        .unwrap_or_default();
    for index in 0..MAX_TILES {
        let region = atlas.regions.get(index).copied().unwrap_or(missing);
        let uv = region.uv_rect(atlas.width, atlas.height);
        let offset = index * 4;
        for component in 0..4 {
            ATLAS_UV_BITS[offset + component].store(uv[component].to_bits(), Ordering::Relaxed);
        }
    }
}

/// Look up an atlas index by texture name (for example `stone` or `blocks/stone`).
pub fn tex_idx(name: &str) -> usize {
    TEXTURE_MAP
        .lock()
        .ok()
        .and_then(|map| {
            map.as_ref().and_then(|map| {
                map.get(name)
                    .or_else(|| map.get(name.strip_prefix("blocks/").unwrap_or(name)))
                    .or_else(|| map.get("__missing"))
                    .copied()
            })
        })
        .unwrap_or(MISSING_TILE_INDEX)
}

#[derive(Deserialize, Debug)]
pub struct AnimationMeta {
    pub animation: AnimationData,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct AnimationData {
    pub frametime: Option<u32>,
    pub interpolate: Option<bool>,
    pub frames: Option<Vec<serde_json::Value>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct AnimatedTexture {
    pub frames: Vec<Vec<u8>>,
    pub normal_frames: Option<Vec<Vec<u8>>>,
    pub specular_frames: Option<Vec<Vec<u8>>>,
    pub frame_times: Vec<u32>,
    pub width: u32,
    pub height: u32,
    pub frametime: u32,
    pub interpolate: bool,
    kind: AnimationKind,
    current_frame: u32,
    current_angle: f64,
    angle_delta: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AnimationKind {
    #[default]
    Generic,
    Clock,
    Compass,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextureAnimationContext {
    pub day_time: i64,
    pub dimension: i8,
    pub player_x: f64,
    pub player_z: f64,
    pub player_yaw_degrees: f64,
    pub spawn_position: Option<[i32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasAnimationUpload {
    pub atlas_index: usize,
    pub frame_index: u32,
    pub pixel_x: u32,
    pub pixel_y: u32,
    pub width: u32,
    pub height: u32,
    pub buffer_offset: u64,
    pub normal_buffer_offset: Option<u64>,
    pub specular_buffer_offset: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct TileInfo {
    pub texture_name: String,
    pub is_animated: bool,
    pub frame_count: u32,
}

pub type UvRect = [f32; 4];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtlasRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl AtlasRegion {
    fn uv_rect(self, atlas_width: u32, atlas_height: u32) -> UvRect {
        if self.width == 0 || self.height == 0 || atlas_width == 0 || atlas_height == 0 {
            return [0.0; 4];
        }
        [
            self.x as f32 / atlas_width as f32,
            self.y as f32 / atlas_height as f32,
            (self.x + self.width) as f32 / atlas_width as f32,
            (self.y + self.height) as f32 / atlas_height as f32,
        ]
    }
}

pub struct TextureAtlas {
    pub pixels: Vec<u8>,
    /// OptiFine/Iris normal-map atlas. Every sprite occupies the exact same
    /// region as `pixels`; missing `_n` companions use Iris' flat-normal
    /// fallback `(127, 127, 255, 255)`.
    pub normal_pixels: Vec<u8>,
    /// OptiFine/Iris specular-map atlas. Missing `_s` companions are fully
    /// transparent black, matching the shader-pack texture-atlas contract.
    pub specular_pixels: Vec<u8>,
    pub animated: HashMap<usize, AnimatedTexture>,
    pub tiles: HashMap<usize, TileInfo>,
    pub name_to_index: HashMap<String, usize>,
    pub regions: Vec<AtlasRegion>,
    pub tick: u32,
    pub width: u32,
    pub height: u32,
}

struct PendingTexture {
    index: usize,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl TextureAtlas {
    pub fn load_with_resolver(resolver: &mut AssetResolver) -> Self {
        let mut pending = Vec::new();
        let mut normal_maps = HashMap::<usize, Vec<u8>>::new();
        let mut specular_maps = HashMap::<usize, Vec<u8>>::new();
        let mut animated = HashMap::new();
        let mut tiles = HashMap::new();
        let mut name_to_index = HashMap::new();
        let mut next_index = 0usize;

        let mut item_texture_names = resolver.list_textures("items");
        item_texture_names.retain(|name| !is_pbr_companion_filename(name));
        item_texture_names.sort_unstable();
        for tex_name in item_texture_names {
            if next_index >= CONTENT_TILE_LIMIT {
                break;
            }
            let path = format!("minecraft/textures/items/{tex_name}");
            let name = strip_png_ext(&tex_name);
            if load_atlas_texture(
                resolver,
                &path,
                name,
                next_index,
                &mut pending,
                &mut tiles,
                &mut name_to_index,
                &mut animated,
                &mut normal_maps,
                &mut specular_maps,
            ) {
                next_index += 1;
            }
        }

        let mut block_texture_names = resolver.list_textures("blocks");
        block_texture_names.retain(|name| !is_pbr_companion_filename(name));
        block_texture_names.sort_unstable();
        for tex_name in block_texture_names {
            if next_index >= CONTENT_TILE_LIMIT {
                log::warn!(
                    "texture atlas full at {} tiles; skipping remaining block textures",
                    MAX_TILES
                );
                break;
            }
            let path = format!("minecraft/textures/blocks/{tex_name}");
            let name = strip_png_ext(&tex_name);
            if load_atlas_texture(
                resolver,
                &path,
                name,
                next_index,
                &mut pending,
                &mut tiles,
                &mut name_to_index,
                &mut animated,
                &mut normal_maps,
                &mut specular_maps,
            ) {
                next_index += 1;
            }
        }

        // EntityRenderer.renderRainSnow uses environment/rain.png. Include it
        // in the world atlas so weather particles share the normal render path.
        if next_index < CONTENT_TILE_LIMIT {
            if let Some(image) =
                decode_resolved_image(resolver, "minecraft/textures/environment/rain.png")
            {
                add_pending_texture(
                    &mut pending,
                    &mut tiles,
                    &mut name_to_index,
                    next_index,
                    "environment/rain".to_string(),
                    image.to_rgba8(),
                    false,
                    1,
                );
                next_index += 1;
            }
        }

        // Chest textures use a 64x64 vanilla layout. Scale every UV island by
        // the resource pack's actual dimensions before placing it in a 16x16
        // logical canvas so the existing ModelChest face UVs remain correct.
        for (variant, relative_path) in [
            ("normal", "chest/normal.png"),
            ("trapped", "chest/trapped.png"),
            ("ender", "chest/ender.png"),
        ] {
            let path = format!("minecraft/textures/entity/{relative_path}");
            let Some(image) = decode_resolved_image(resolver, &path) else {
                continue;
            };
            for (part, x, y, width, height) in chest_face_regions() {
                if next_index >= CONTENT_TILE_LIMIT {
                    break;
                }
                let face = extract_scaled_chest_face(&image, x, y, width, height);
                let name = format!("chest_{variant}_{part}");
                add_pending_texture(
                    &mut pending,
                    &mut tiles,
                    &mut name_to_index,
                    next_index,
                    name,
                    face,
                    false,
                    1,
                );
                next_index += 1;
            }
        }

        for (variant, relative_path) in [
            ("normal", "chest/normal_double.png"),
            ("trapped", "chest/trapped_double.png"),
        ] {
            let path = format!("minecraft/textures/entity/{relative_path}");
            let Some(image) = decode_resolved_image(resolver, &path) else {
                continue;
            };
            for (part, texture_x, texture_y, width, height, depth) in [
                ("lid", 0, 0, 30, 5, 14),
                ("body", 0, 19, 30, 10, 14),
                ("knob", 0, 0, 2, 4, 1),
            ] {
                for (face, x, y, face_width, face_height) in
                    model_box_face_regions(texture_x, texture_y, width, height, depth)
                {
                    if next_index >= CONTENT_TILE_LIMIT {
                        break;
                    }
                    add_pending_texture(
                        &mut pending,
                        &mut tiles,
                        &mut name_to_index,
                        next_index,
                        format!("chest_{variant}_double_{part}_{face}"),
                        extract_scaled_model_face(&image, x, y, face_width, face_height, 128, 64),
                        false,
                        1,
                    );
                    next_index += 1;
                }
            }
        }

        // TileEntitySignRenderer binds entity/sign.png. ModelSign uses two
        // ModelBox instances, so preserve their exact per-face UV islands
        // instead of falling back to the plank block texture.
        if let Some(image) = decode_resolved_image(resolver, "minecraft/textures/entity/sign.png") {
            for (part, texture_x, texture_y, width, height, depth) in
                [("board", 0, 0, 24, 12, 2), ("stick", 0, 14, 2, 14, 2)]
            {
                for (face, x, y, face_width, face_height) in
                    model_box_face_regions(texture_x, texture_y, width, height, depth)
                {
                    if next_index >= CONTENT_TILE_LIMIT {
                        break;
                    }
                    let face_image =
                        extract_scaled_model_face(&image, x, y, face_width, face_height, 64, 32);
                    add_pending_texture(
                        &mut pending,
                        &mut tiles,
                        &mut name_to_index,
                        next_index,
                        format!("sign_{part}_{face}"),
                        face_image,
                        false,
                        1,
                    );
                    next_index += 1;
                }
            }
        }

        if let Some(image) =
            decode_resolved_image(resolver, "minecraft/textures/particle/particles.png")
        {
            let (width, height) = image.dimensions();
            if width % 16 == 0 && height % 16 == 0 {
                let sprite_width = width / 16;
                let sprite_height = height / 16;
                for row in 0..16 {
                    for column in 0..16 {
                        if next_index >= CONTENT_TILE_LIMIT {
                            break;
                        }
                        let sprite = image
                            .crop_imm(
                                column * sprite_width,
                                row * sprite_height,
                                sprite_width,
                                sprite_height,
                            )
                            .to_rgba8();
                        add_pending_texture(
                            &mut pending,
                            &mut tiles,
                            &mut name_to_index,
                            next_index,
                            format!("particle_{}", row * 16 + column),
                            sprite,
                            false,
                            1,
                        );
                        next_index += 1;
                    }
                }
                log::info!("loaded 256 particle sprites at {sprite_width}x{sprite_height}");
            }
        }

        if let Some(image) =
            decode_resolved_image(resolver, "minecraft/textures/entity/explosion.png")
        {
            let (width, height) = image.dimensions();
            if width == height && width % 4 == 0 {
                let frame_size = width / 4;
                for frame in 0..16u32 {
                    if next_index >= CONTENT_TILE_LIMIT {
                        break;
                    }
                    let frame_image = image
                        .crop_imm(
                            frame % 4 * frame_size,
                            frame / 4 * frame_size,
                            frame_size,
                            frame_size,
                        )
                        .to_rgba8();
                    add_pending_texture(
                        &mut pending,
                        &mut tiles,
                        &mut name_to_index,
                        next_index,
                        format!("explosion_{frame}"),
                        frame_image,
                        false,
                        1,
                    );
                    next_index += 1;
                }
            }
        }

        let mut missing = RgbaImage::new(TILE_SIZE, TILE_SIZE);
        for (x, y, pixel) in missing.enumerate_pixels_mut() {
            *pixel = if (x / 4 + y / 4) % 2 == 0 {
                image::Rgba([255, 0, 255, 255])
            } else {
                image::Rgba([24, 24, 24, 255])
            };
        }
        add_pending_texture(
            &mut pending,
            &mut tiles,
            &mut name_to_index,
            MISSING_TILE_INDEX,
            "__missing".into(),
            missing,
            false,
            1,
        );
        add_pending_texture(
            &mut pending,
            &mut tiles,
            &mut name_to_index,
            WHITE_TILE_INDEX,
            "__white".into(),
            RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgba([255, 255, 255, 255])),
            false,
            1,
        );

        let (width, height, mut regions) = pack_regions(&pending);
        let missing_region = regions[MISSING_TILE_INDEX];
        for region in &mut regions {
            if region.width == 0 || region.height == 0 {
                *region = missing_region;
            }
        }
        let mut pixels = vec![0; (width * height * 4) as usize];
        for texture in &pending {
            copy_sprite_with_gutter(&mut pixels, width, regions[texture.index], &texture.pixels);
        }
        let normal_pixels = build_companion_atlas(
            width,
            height,
            &pending,
            &regions,
            &normal_maps,
            [127, 127, 255, 255],
        );
        let specular_pixels = build_companion_atlas(
            width,
            height,
            &pending,
            &regions,
            &specular_maps,
            [0, 0, 0, 0],
        );

        log::info!(
            "texture atlas ready: textures={}, animated={}, normal_maps={}, specular_maps={}, dimensions={}x{}",
            name_to_index.len(),
            animated.len(),
            normal_maps.len(),
            specular_maps.len(),
            width,
            height,
        );

        Self {
            pixels,
            normal_pixels,
            specular_pixels,
            animated,
            tiles,
            name_to_index,
            regions,
            tick: 0,
            width,
            height,
        }
    }

    pub fn load(base: &str) -> Self {
        let mut resolver = AssetResolver::new(base);
        Self::load_with_resolver(&mut resolver)
    }

    pub fn animate_tick_into(
        &mut self,
        upload_bytes: &mut Vec<u8>,
        uploads: &mut Vec<AtlasAnimationUpload>,
    ) {
        self.animate_tick_with_context_into(
            TextureAnimationContext::default(),
            upload_bytes,
            uploads,
        );
    }

    pub fn animate_tick_with_context_into(
        &mut self,
        context: TextureAnimationContext,
        upload_bytes: &mut Vec<u8>,
        uploads: &mut Vec<AtlasAnimationUpload>,
    ) {
        self.tick = self.tick.wrapping_add(1);

        for (index, animation) in &mut self.animated {
            let previous_frame = animation.current_frame;
            let frame_index = animation_frame_for_context(self.tick, animation, context);
            if frame_index == previous_frame {
                continue;
            }
            animation.current_frame = frame_index;
            let Some(frame) = animation.frames.get(frame_index as usize) else {
                continue;
            };
            if frame.len() != (animation.width * animation.height * 4) as usize {
                continue;
            }
            let Some(region) = self.regions.get(*index).copied() else {
                continue;
            };
            if region.width != animation.width || region.height != animation.height {
                continue;
            }

            let buffer_offset = upload_bytes.len() as u64;
            append_sprite_with_gutter(upload_bytes, region, frame);
            let normal_buffer_offset = animation
                .normal_frames
                .as_ref()
                .and_then(|frames| frames.get(frame_index as usize))
                .filter(|frame| frame.len() == (animation.width * animation.height * 4) as usize)
                .map(|frame| {
                    let offset = upload_bytes.len() as u64;
                    append_sprite_with_gutter(upload_bytes, region, frame);
                    copy_sprite_with_gutter(&mut self.normal_pixels, self.width, region, frame);
                    offset
                });
            let specular_buffer_offset = animation
                .specular_frames
                .as_ref()
                .and_then(|frames| frames.get(frame_index as usize))
                .filter(|frame| frame.len() == (animation.width * animation.height * 4) as usize)
                .map(|frame| {
                    let offset = upload_bytes.len() as u64;
                    append_sprite_with_gutter(upload_bytes, region, frame);
                    copy_sprite_with_gutter(&mut self.specular_pixels, self.width, region, frame);
                    offset
                });
            uploads.push(AtlasAnimationUpload {
                atlas_index: *index,
                frame_index,
                pixel_x: region.x - ATLAS_PADDING,
                pixel_y: region.y - ATLAS_PADDING,
                width: region.width + ATLAS_PADDING * 2,
                height: region.height + ATLAS_PADDING * 2,
                buffer_offset,
                normal_buffer_offset,
                specular_buffer_offset,
            });
            copy_sprite_with_gutter(&mut self.pixels, self.width, region, frame);
        }
    }

    pub fn tile_index(&self, name: &str) -> usize {
        self.name_to_index
            .get(name)
            .copied()
            .unwrap_or(MISSING_TILE_INDEX)
    }

    pub fn uv_rect(&self, name: &str) -> UvRect {
        self.uv_for_index(self.tile_index(name))
    }

    pub fn uv_for_index(&self, index: usize) -> UvRect {
        self.regions
            .get(index)
            .or_else(|| self.regions.get(MISSING_TILE_INDEX))
            .copied()
            .unwrap_or_default()
            .uv_rect(self.width, self.height)
    }
}

fn decode_resolved_image(resolver: &mut AssetResolver, path: &str) -> Option<DynamicImage> {
    let bytes = resolver.read_bytes(path)?;
    let image = match decode_image_with_limits(&bytes, MAX_ATLAS_DIMENSION, 512 * 1024 * 1024) {
        Ok(image) => image,
        Err(error) => {
            log::warn!("failed to decode texture '{path}': {error}");
            return None;
        }
    };
    let (w, h) = image.dimensions();
    if w > MAX_TEX_DIM || h > MAX_TEX_DIM {
        let ratio = MAX_TEX_DIM as f64 / (w.max(h) as f64);
        let new_w = (w as f64 * ratio).ceil() as u32;
        let new_h = (h as f64 * ratio).ceil() as u32;
        log::warn!("texture '{path}' is {w}×{h}, downscaled to {new_w}×{new_h}");
        Some(image.resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3))
    } else {
        Some(image)
    }
}

/// Load a texture without the `MAX_TEX_DIM` cap so animated sprite sheets
/// stay at full resolution until individual frames are extracted.
fn decode_resolved_image_full(resolver: &mut AssetResolver, path: &str) -> Option<DynamicImage> {
    let bytes = resolver.read_bytes(path)?;
    match decode_image_with_limits(&bytes, MAX_ATLAS_DIMENSION, 512 * 1024 * 1024) {
        Ok(image) => Some(image),
        Err(error) => {
            log::warn!("failed to decode texture '{path}': {error}");
            None
        }
    }
}

pub(crate) fn decode_image_with_limits(
    bytes: &[u8],
    max_dimension: u32,
    max_alloc: u64,
) -> image::ImageResult<DynamicImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    limits.max_alloc = Some(max_alloc);
    reader.limits(limits);
    reader.decode()
}

#[allow(clippy::too_many_arguments)]
fn load_atlas_texture(
    resolver: &mut AssetResolver,
    path: &str,
    name: String,
    index: usize,
    pending: &mut Vec<PendingTexture>,
    tiles: &mut HashMap<usize, TileInfo>,
    name_to_index: &mut HashMap<String, usize>,
    animated: &mut HashMap<usize, AnimatedTexture>,
    normal_maps: &mut HashMap<usize, Vec<u8>>,
    specular_maps: &mut HashMap<usize, Vec<u8>>,
) -> bool {
    let meta = resolver
        .read_string(&format!("{path}.mcmeta"))
        .and_then(|json| match parse_animation_meta(&json) {
            Ok(meta) => Some(meta),
            Err(error) => {
                log::warn!("failed to parse animation metadata '{path}.mcmeta': {error}");
                None
            }
        });

    if let Some(meta) = meta {
        let Some(mut animation) = decode_animated_texture(resolver, path, &meta.animation) else {
            return false;
        };
        animation.kind = match name.as_str() {
            "clock" => AnimationKind::Clock,
            "compass" => AnimationKind::Compass,
            _ => AnimationKind::Generic,
        };
        if let Some(first) = animation
            .normal_frames
            .as_ref()
            .and_then(|frames| frames.first())
        {
            normal_maps.insert(index, first.clone());
        }
        if let Some(first) = animation
            .specular_frames
            .as_ref()
            .and_then(|frames| frames.first())
        {
            specular_maps.insert(index, first.clone());
        }
        let Some(first) = animation.frames.first() else {
            return false;
        };
        pending.push(PendingTexture {
            index,
            pixels: first.clone(),
            width: animation.width,
            height: animation.height,
        });
        let frame_count = animation.frames.len() as u32;
        animated.insert(index, animation);
        tiles.insert(
            index,
            TileInfo {
                texture_name: name.clone(),
                is_animated: true,
                frame_count,
            },
        );
        name_to_index.insert(name, index);
        return true;
    }

    let Some(image) = decode_resolved_image(resolver, path) else {
        return false;
    };
    let (width, height) = image.dimensions();
    load_material_companions(
        resolver,
        path,
        width,
        height,
        index,
        normal_maps,
        specular_maps,
    );
    add_pending_texture(
        pending,
        tiles,
        name_to_index,
        index,
        name,
        image.to_rgba8(),
        false,
        1,
    );
    true
}

fn parse_animation_meta(json: &str) -> Result<AnimationMeta, serde_json::Error> {
    serde_json::from_str(json).or_else(|_| {
        let sanitized = strip_json_trailing_commas(json);
        serde_json::from_str(&sanitized)
    })
}

/// Gson accepts trailing commas in resource metadata commonly shipped by
/// older packs. Remove only commas outside strings whose next non-whitespace
/// character closes an array or object.
fn strip_json_trailing_commas(json: &str) -> String {
    let chars = json.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &character) in chars.iter().enumerate() {
        if in_string {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        if character == '"' {
            in_string = true;
            output.push(character);
            continue;
        }
        if character == ',' {
            let next = chars[index + 1..]
                .iter()
                .copied()
                .find(|next| !next.is_whitespace());
            if matches!(next, Some(']') | Some('}')) {
                continue;
            }
        }
        output.push(character);
    }
    output
}

fn decode_animated_texture(
    resolver: &mut AssetResolver,
    path: &str,
    meta: &AnimationData,
) -> Option<AnimatedTexture> {
    // Extract frames before applying the dimension cap. Resizing a complete
    // vertical strip would shrink every individual frame to a few pixels.
    let source_image = decode_resolved_image_full(resolver, path)?;
    let (source_width, source_height) = source_image.dimensions();
    let source_frame_width = meta.width.unwrap_or(source_width).max(1);
    let source_frame_height = meta.height.unwrap_or(source_frame_width).max(1);
    let columns = (source_width / source_frame_width).max(1);
    let rows = (source_height / source_frame_height).max(1);
    let mut physical_frames = Vec::with_capacity((columns * rows) as usize);
    let mut packed_dimensions = None;
    for row in 0..rows {
        for column in 0..columns {
            let frame = source_image.crop_imm(
                column * source_frame_width,
                row * source_frame_height,
                source_frame_width.min(source_width - column * source_frame_width),
                source_frame_height.min(source_height - row * source_frame_height),
            );
            let capped = cap_texture_dimensions(frame);
            packed_dimensions.get_or_insert(capped.dimensions());
            physical_frames.push(capped.to_rgba8().into_raw());
        }
    }
    let (width, height) = packed_dimensions?;

    let physical_normal_frames = load_animated_material_companion(
        resolver,
        path,
        "_n",
        columns,
        rows,
        source_frame_width,
        source_frame_height,
        width,
        height,
    );
    let physical_specular_frames = load_animated_material_companion(
        resolver,
        path,
        "_s",
        columns,
        rows,
        source_frame_width,
        source_frame_height,
        width,
        height,
    );
    let sequence = animation_sequence(meta, physical_frames.len());
    let frames = reorder_animation_frames(&physical_frames, &sequence);
    let normal_frames = physical_normal_frames
        .as_ref()
        .map(|frames| reorder_animation_frames(frames, &sequence));
    let specular_frames = physical_specular_frames
        .as_ref()
        .map(|frames| reorder_animation_frames(frames, &sequence));
    let frame_times = sequence.iter().map(|(_, time)| *time).collect();

    Some(AnimatedTexture {
        frames,
        normal_frames,
        specular_frames,
        frame_times,
        width,
        height,
        frametime: meta.frametime.unwrap_or(1).max(1),
        interpolate: meta.interpolate.unwrap_or(false),
        kind: AnimationKind::Generic,
        current_frame: 0,
        current_angle: 0.0,
        angle_delta: 0.0,
    })
}

fn cap_texture_dimensions(image: DynamicImage) -> DynamicImage {
    if image.width() <= MAX_TEX_DIM && image.height() <= MAX_TEX_DIM {
        return image;
    }
    let ratio = MAX_TEX_DIM as f64 / image.width().max(image.height()) as f64;
    let width = (image.width() as f64 * ratio).ceil() as u32;
    let height = (image.height() as f64 * ratio).ceil() as u32;
    image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
}

fn animation_sequence(meta: &AnimationData, physical_frame_count: usize) -> Vec<(usize, u32)> {
    let default_time = meta.frametime.unwrap_or(1).max(1);
    let mut sequence = meta
        .frames
        .as_ref()
        .map(|frames| {
            frames
                .iter()
                .filter_map(|frame| animation_frame_spec(frame, default_time))
                .filter(|(index, _)| *index < physical_frame_count)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if sequence.is_empty() {
        sequence.extend((0..physical_frame_count).map(|index| (index, default_time)));
    }
    sequence
}

fn animation_frame_spec(value: &serde_json::Value, default_time: u32) -> Option<(usize, u32)> {
    if let Some(index) = value.as_u64().and_then(|index| usize::try_from(index).ok()) {
        return Some((index, default_time));
    }
    let frame = value.as_object()?;
    let index = frame
        .get("index")?
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())?;
    let time = frame
        .get("time")
        .and_then(serde_json::Value::as_u64)
        .and_then(|time| u32::try_from(time).ok())
        .unwrap_or(default_time)
        .max(1);
    Some((index, time))
}

fn reorder_animation_frames(frames: &[Vec<u8>], sequence: &[(usize, u32)]) -> Vec<Vec<u8>> {
    sequence
        .iter()
        .filter_map(|(index, _)| frames.get(*index).cloned())
        .collect()
}

fn is_pbr_companion_filename(name: &str) -> bool {
    let stem = name.strip_suffix(".png").unwrap_or(name);
    stem.ends_with("_n") || stem.ends_with("_s")
}

#[allow(clippy::too_many_arguments)]
fn load_material_companions(
    resolver: &mut AssetResolver,
    base_path: &str,
    target_width: u32,
    target_height: u32,
    index: usize,
    normal_maps: &mut HashMap<usize, Vec<u8>>,
    specular_maps: &mut HashMap<usize, Vec<u8>>,
) {
    if let Some(pixels) =
        load_material_companion(resolver, base_path, "_n", target_width, target_height)
    {
        normal_maps.insert(index, pixels);
    }
    if let Some(pixels) =
        load_material_companion(resolver, base_path, "_s", target_width, target_height)
    {
        specular_maps.insert(index, pixels);
    }
}

#[allow(clippy::too_many_arguments)]
fn load_material_companion(
    resolver: &mut AssetResolver,
    base_path: &str,
    suffix: &str,
    target_width: u32,
    target_height: u32,
) -> Option<Vec<u8>> {
    let stem = base_path.strip_suffix(".png").unwrap_or(base_path);
    let path = format!("{stem}{suffix}.png");
    let image = decode_resolved_image_full(resolver, &path)?;
    let image = if image.width() == target_width && image.height() == target_height {
        image
    } else {
        log::warn!(
            "PBR companion '{path}' is {}x{}, resized to {}x{} to match its atlas sprite",
            image.width(),
            image.height(),
            target_width,
            target_height
        );
        image.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        )
    };
    Some(image.to_rgba8().into_raw())
}

#[allow(clippy::too_many_arguments)]
fn load_animated_material_companion(
    resolver: &mut AssetResolver,
    base_path: &str,
    suffix: &str,
    columns: u32,
    rows: u32,
    source_frame_width: u32,
    source_frame_height: u32,
    target_width: u32,
    target_height: u32,
) -> Option<Vec<Vec<u8>>> {
    let stem = base_path.strip_suffix(".png").unwrap_or(base_path);
    let path = format!("{stem}{suffix}.png");
    let image = decode_resolved_image_full(resolver, &path)?;
    let frame_count = (columns * rows).max(1) as usize;
    let candidate_width = image.width() / columns.max(1);
    let candidate_height = image.height() / rows.max(1);
    let has_matching_sheet = candidate_width > 0
        && candidate_height > 0
        && image.width() % columns.max(1) == 0
        && image.height() % rows.max(1) == 0
        && candidate_width as u64 * source_frame_height as u64
            == candidate_height as u64 * source_frame_width as u64;

    let resize_frame = |frame: DynamicImage| {
        let frame = if frame.width() == target_width && frame.height() == target_height {
            frame
        } else {
            frame.resize_exact(
                target_width,
                target_height,
                image::imageops::FilterType::Triangle,
            )
        };
        frame.to_rgba8().into_raw()
    };

    if !has_matching_sheet {
        log::warn!(
            "animated PBR companion '{path}' does not match the base sprite sheet; repeating it for all {frame_count} frames"
        );
        let frame = resize_frame(image);
        return Some(vec![frame; frame_count]);
    }

    let mut frames = Vec::with_capacity(frame_count);
    for row in 0..rows {
        for column in 0..columns {
            frames.push(resize_frame(image.crop_imm(
                column * candidate_width,
                row * candidate_height,
                candidate_width,
                candidate_height,
            )));
        }
    }
    Some(frames)
}

fn build_companion_atlas(
    width: u32,
    height: u32,
    pending: &[PendingTexture],
    regions: &[AtlasRegion],
    maps: &HashMap<usize, Vec<u8>>,
    fallback: [u8; 4],
) -> Vec<u8> {
    let mut pixels = fallback.repeat((width * height) as usize);
    for texture in pending {
        let Some(companion) = maps.get(&texture.index) else {
            continue;
        };
        if companion.len() != (texture.width * texture.height * 4) as usize {
            log::warn!(
                "PBR companion for atlas tile {} has {} bytes, expected {}; using fallback",
                texture.index,
                companion.len(),
                texture.width * texture.height * 4
            );
            continue;
        }
        let Some(region) = regions.get(texture.index).copied() else {
            continue;
        };
        copy_sprite_with_gutter(&mut pixels, width, region, companion);
    }
    pixels
}

fn add_pending_texture(
    pending: &mut Vec<PendingTexture>,
    tiles: &mut HashMap<usize, TileInfo>,
    names: &mut HashMap<String, usize>,
    index: usize,
    name: String,
    image: RgbaImage,
    is_animated: bool,
    frame_count: u32,
) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    pending.push(PendingTexture {
        index,
        pixels: image.into_raw(),
        width,
        height,
    });
    tiles.insert(
        index,
        TileInfo {
            texture_name: name.clone(),
            is_animated,
            frame_count,
        },
    );
    names.insert(name, index);
}

fn chest_face_regions() -> [(&'static str, u32, u32, u32, u32); 18] {
    [
        ("body_top", 28, 19, 14, 14),
        ("body_bottom", 14, 19, 14, 14),
        ("body_north", 14, 33, 14, 10),
        ("body_south", 42, 33, 14, 10),
        ("body_west", 0, 33, 14, 10),
        ("body_east", 28, 33, 14, 10),
        ("lid_top", 28, 0, 14, 14),
        ("lid_bottom", 14, 0, 14, 14),
        ("lid_north", 14, 14, 14, 5),
        ("lid_south", 42, 14, 14, 5),
        ("lid_west", 0, 14, 14, 5),
        ("lid_east", 28, 14, 14, 5),
        ("knob_top", 3, 0, 2, 1),
        ("knob_bottom", 1, 0, 2, 1),
        ("knob_north", 1, 1, 2, 4),
        ("knob_south", 4, 1, 2, 4),
        ("knob_west", 0, 1, 1, 4),
        ("knob_east", 3, 1, 1, 4),
    ]
}

fn model_box_face_regions(
    texture_x: u32,
    texture_y: u32,
    width: u32,
    height: u32,
    depth: u32,
) -> [(&'static str, u32, u32, u32, u32); 6] {
    [
        ("top", texture_x + depth + width, texture_y, width, depth),
        ("bottom", texture_x + depth, texture_y, width, depth),
        ("north", texture_x + depth, texture_y + depth, width, height),
        (
            "south",
            texture_x + depth + width + depth,
            texture_y + depth,
            width,
            height,
        ),
        ("west", texture_x, texture_y + depth, depth, height),
        (
            "east",
            texture_x + depth + width,
            texture_y + depth,
            depth,
            height,
        ),
    ]
}

fn extract_scaled_model_face(
    image: &DynamicImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    texture_width: u32,
    texture_height: u32,
) -> RgbaImage {
    let (source_width, source_height) = image.dimensions();
    let x0 = x * source_width / texture_width;
    let y0 = y * source_height / texture_height;
    let x1 = (x + width) * source_width / texture_width;
    let y1 = (y + height) * source_height / texture_height;
    image
        .crop_imm(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
        .to_rgba8()
}

fn extract_scaled_chest_face(
    image: &DynamicImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> RgbaImage {
    let (source_width, source_height) = image.dimensions();
    let x0 = x * source_width / 64;
    let y0 = y * source_height / 64;
    let x1 = (x + width) * source_width / 64;
    let y1 = (y + height) * source_height / 64;
    let canvas_width = (TILE_SIZE * source_width / 64).max(1);
    let canvas_height = (TILE_SIZE * source_height / 64).max(1);
    let mut canvas = RgbaImage::new(canvas_width, canvas_height);
    let face = image
        .crop_imm(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
        .to_rgba8();
    image::imageops::replace(&mut canvas, &face, 0, 0);
    canvas
}

fn pack_regions(textures: &[PendingTexture]) -> (u32, u32, Vec<AtlasRegion>) {
    let total_area: u64 = textures
        .iter()
        .map(|texture| {
            (texture.width + ATLAS_PADDING * 2) as u64 * (texture.height + ATLAS_PADDING * 2) as u64
        })
        .sum();
    let widest = textures
        .iter()
        .map(|texture| texture.width + ATLAS_PADDING * 2)
        .max()
        .unwrap_or(1);
    let approximate_side = (total_area as f64).sqrt().ceil() as u32;
    let mut atlas_width = approximate_side.max(widest).max(64).next_power_of_two();

    loop {
        assert!(
            atlas_width <= MAX_ATLAS_DIMENSION,
            "texture pack requires an atlas wider than {MAX_ATLAS_DIMENSION}px"
        );
        if let Some((needed_height, regions)) = try_pack_at_width(textures, atlas_width) {
            let atlas_height = needed_height.max(1).next_power_of_two();
            if atlas_height <= MAX_ATLAS_DIMENSION {
                return (atlas_width, atlas_height, regions);
            }
        }
        atlas_width *= 2;
    }
}

fn try_pack_at_width(
    textures: &[PendingTexture],
    atlas_width: u32,
) -> Option<(u32, Vec<AtlasRegion>)> {
    let mut order: Vec<_> = textures.iter().collect();
    order.sort_unstable_by(|left, right| {
        right
            .height
            .cmp(&left.height)
            .then_with(|| right.width.cmp(&left.width))
            .then_with(|| left.index.cmp(&right.index))
    });
    let mut regions = vec![AtlasRegion::default(); MAX_TILES];
    let mut x = ATLAS_PADDING;
    let mut y = ATLAS_PADDING;
    let mut shelf_height = 0;

    for texture in order {
        let packed_width = texture.width + ATLAS_PADDING * 2;
        let packed_height = texture.height + ATLAS_PADDING * 2;
        if packed_width > atlas_width {
            return None;
        }
        if x + texture.width + ATLAS_PADDING > atlas_width {
            x = ATLAS_PADDING;
            y += shelf_height;
            shelf_height = 0;
        }
        if y + packed_height > MAX_ATLAS_DIMENSION {
            return None;
        }
        regions[texture.index] = AtlasRegion {
            x,
            y,
            width: texture.width,
            height: texture.height,
        };
        x += packed_width;
        shelf_height = shelf_height.max(packed_height);
    }

    Some((y + shelf_height + ATLAS_PADDING, regions))
}

fn copy_sprite_with_gutter(atlas: &mut [u8], atlas_width: u32, region: AtlasRegion, sprite: &[u8]) {
    for offset_y in -1i32..=region.height as i32 {
        for offset_x in -1i32..=region.width as i32 {
            let source_x = offset_x.clamp(0, region.width as i32 - 1) as u32;
            let source_y = offset_y.clamp(0, region.height as i32 - 1) as u32;
            let destination_x = (region.x as i32 + offset_x) as u32;
            let destination_y = (region.y as i32 + offset_y) as u32;
            let source = ((source_y * region.width + source_x) * 4) as usize;
            let destination = ((destination_y * atlas_width + destination_x) * 4) as usize;
            if source + 4 <= sprite.len() && destination + 4 <= atlas.len() {
                atlas[destination..destination + 4].copy_from_slice(&sprite[source..source + 4]);
            }
        }
    }
}

fn append_sprite_with_gutter(output: &mut Vec<u8>, region: AtlasRegion, sprite: &[u8]) {
    for offset_y in -1i32..=region.height as i32 {
        for offset_x in -1i32..=region.width as i32 {
            let source_x = offset_x.clamp(0, region.width as i32 - 1) as u32;
            let source_y = offset_y.clamp(0, region.height as i32 - 1) as u32;
            let source = ((source_y * region.width + source_x) * 4) as usize;
            if source + 4 <= sprite.len() {
                output.extend_from_slice(&sprite[source..source + 4]);
            }
        }
    }
}

fn frame_at_tick(tick: u32, animation: &AnimatedTexture) -> u32 {
    if animation.frames.is_empty() {
        return 0;
    }
    if animation.frame_times.len() != animation.frames.len() {
        let total_ticks = animation.frames.len() as u32 * animation.frametime;
        return (tick % total_ticks.max(1)) / animation.frametime.max(1);
    }

    let total_ticks = animation
        .frame_times
        .iter()
        .fold(0u64, |total, time| total.saturating_add(u64::from(*time)));
    let mut remaining = u64::from(tick) % total_ticks.max(1);
    for (index, time) in animation.frame_times.iter().enumerate() {
        if remaining < u64::from(*time) {
            return index as u32;
        }
        remaining -= u64::from(*time);
    }
    animation.frames.len().saturating_sub(1) as u32
}

fn animation_frame_for_context(
    tick: u32,
    animation: &mut AnimatedTexture,
    context: TextureAnimationContext,
) -> u32 {
    match animation.kind {
        AnimationKind::Generic => frame_at_tick(tick, animation),
        AnimationKind::Clock => clock_frame(animation, context),
        AnimationKind::Compass => compass_frame(animation, context),
    }
}

fn clock_frame(animation: &mut AnimatedTexture, context: TextureAnimationContext) -> u32 {
    let frame_count = animation.frames.len();
    if frame_count == 0 {
        return 0;
    }
    let target = if context.dimension == 0 {
        f64::from(renderer_api::core::sky::SkyGradient::celestial_angle(
            context.day_time as f32,
        ))
    } else {
        rand::random::<f64>()
    };
    let mut difference = target - animation.current_angle;
    while difference < -0.5 {
        difference += 1.0;
    }
    while difference >= 0.5 {
        difference -= 1.0;
    }
    animation.angle_delta += difference.clamp(-1.0, 1.0) * 0.1;
    animation.angle_delta *= 0.8;
    animation.current_angle += animation.angle_delta;
    (((animation.current_angle + 1.0) * frame_count as f64) as i32).rem_euclid(frame_count as i32)
        as u32
}

fn compass_frame(animation: &mut AnimatedTexture, context: TextureAnimationContext) -> u32 {
    let frame_count = animation.frames.len();
    if frame_count == 0 {
        return 0;
    }
    let target = if context.dimension == 0 {
        if let Some(spawn) = context.spawn_position {
            let dx = f64::from(spawn[0]) - context.player_x;
            let dz = f64::from(spawn[2]) - context.player_z;
            -((context.player_yaw_degrees.rem_euclid(360.0) - 90.0).to_radians() - dz.atan2(dx))
        } else {
            0.0
        }
    } else {
        rand::random::<f64>() * std::f64::consts::TAU
    };
    let mut difference = target - animation.current_angle;
    while difference < -std::f64::consts::PI {
        difference += std::f64::consts::TAU;
    }
    while difference >= std::f64::consts::PI {
        difference -= std::f64::consts::TAU;
    }
    animation.angle_delta += difference.clamp(-1.0, 1.0) * 0.1;
    animation.angle_delta *= 0.8;
    animation.current_angle += animation.angle_delta;
    (((animation.current_angle / std::f64::consts::TAU + 1.0) * frame_count as f64) as i32)
        .rem_euclid(frame_count as i32) as u32
}

pub fn tile_uv_rect(index: usize) -> UvRect {
    let index = if index < MAX_TILES {
        index
    } else {
        MISSING_TILE_INDEX
    };
    let offset = index * 4;
    let uv = [
        f32::from_bits(ATLAS_UV_BITS[offset].load(Ordering::Relaxed)),
        f32::from_bits(ATLAS_UV_BITS[offset + 1].load(Ordering::Relaxed)),
        f32::from_bits(ATLAS_UV_BITS[offset + 2].load(Ordering::Relaxed)),
        f32::from_bits(ATLAS_UV_BITS[offset + 3].load(Ordering::Relaxed)),
    ];
    if uv[2] > uv[0] && uv[3] > uv[1] {
        uv
    } else {
        legacy_tile_uv_rect(index)
    }
}

fn legacy_tile_uv_rect(index: usize) -> UvRect {
    let x = index as u32 % ATLAS_TILES_X;
    let y = index as u32 / ATLAS_TILES_X;
    [
        x as f32 / ATLAS_TILES_X as f32,
        y as f32 / ATLAS_TILES_Y as f32,
        (x + 1) as f32 / ATLAS_TILES_X as f32,
        (y + 1) as f32 / ATLAS_TILES_Y as f32,
    ]
}

pub fn tile_uv(index: usize) -> ([f32; 2], [f32; 2]) {
    let region = tile_uv_rect(index);
    ([region[0], region[1]], [region[2], region[3]])
}

fn strip_png_ext(name: &str) -> String {
    name.strip_suffix(".png").unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animation_atlas(width: u32, height: u32) -> TextureAtlas {
        let index = 7;
        let frame_len = (width * height * 4) as usize;
        let region = AtlasRegion {
            x: 3,
            y: 5,
            width,
            height,
        };
        let mut regions = vec![region; MAX_TILES];
        regions[index] = region;
        TextureAtlas {
            pixels: vec![0; (32 * 32 * 4) as usize],
            normal_pixels: [127, 127, 255, 255].repeat(32 * 32),
            specular_pixels: [0, 0, 0, 0].repeat(32 * 32),
            animated: HashMap::from([(
                index,
                AnimatedTexture {
                    frames: vec![vec![0; frame_len], vec![7; frame_len]],
                    normal_frames: None,
                    specular_frames: None,
                    frame_times: vec![2, 2],
                    width,
                    height,
                    frametime: 2,
                    interpolate: false,
                    kind: AnimationKind::Generic,
                    current_frame: 0,
                    current_angle: 0.0,
                    angle_delta: 0.0,
                },
            )]),
            tiles: HashMap::new(),
            name_to_index: HashMap::new(),
            regions,
            tick: 0,
            width: 32,
            height: 32,
        }
    }

    #[test]
    fn animation_metadata_controls_frame_order_and_individual_times() {
        let meta = AnimationData {
            frametime: Some(2),
            frames: Some(vec![
                serde_json::json!(0),
                serde_json::json!({ "index": 2, "time": 3 }),
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!(99),
            ]),
            ..AnimationData::default()
        };
        assert_eq!(
            animation_sequence(&meta, 3),
            vec![(0, 2), (2, 3), (1, 2), (2, 2)]
        );

        let mut atlas = animation_atlas(8, 8);
        let animation = atlas.animated.get_mut(&7).unwrap();
        animation.frames = vec![vec![0; 8 * 8 * 4]; 4];
        animation.frame_times = vec![2, 3, 2, 2];
        assert_eq!(frame_at_tick(0, animation), 0);
        assert_eq!(frame_at_tick(2, animation), 1);
        assert_eq!(frame_at_tick(5, animation), 2);
        assert_eq!(frame_at_tick(7, animation), 3);
        assert_eq!(frame_at_tick(9, animation), 0);
    }

    #[test]
    fn animation_metadata_accepts_gson_style_trailing_commas_only_outside_strings() {
        let meta = parse_animation_meta(
            r#"{
                "animation": {
                    "frames": [0, 1, 2,],
                    "frametime": 2,
                    "interpolate": false,
                }
            }"#,
        )
        .unwrap();
        assert_eq!(meta.animation.frames.unwrap().len(), 3);
        assert_eq!(meta.animation.frametime, Some(2));
        assert_eq!(
            strip_json_trailing_commas(r#"{"value": ",}"}"#),
            r#"{"value": ",}"}"#
        );
    }

    #[test]
    fn animation_upload_preserves_native_frame_dimensions() {
        let mut atlas = animation_atlas(8, 12);
        let mut bytes = Vec::new();
        let mut uploads = Vec::new();

        atlas.animate_tick_into(&mut bytes, &mut uploads);
        assert!(uploads.is_empty());
        atlas.animate_tick_into(&mut bytes, &mut uploads);

        assert_eq!(bytes.len(), 10 * 14 * 4);
        assert_eq!(
            uploads,
            vec![AtlasAnimationUpload {
                atlas_index: 7,
                frame_index: 1,
                pixel_x: 2,
                pixel_y: 4,
                width: 10,
                height: 14,
                buffer_offset: 0,
                normal_buffer_offset: None,
                specular_buffer_offset: None,
            }]
        );
        let atlas_offset = ((5 * 32 + 3) * 4) as usize;
        assert_eq!(&atlas.pixels[atlas_offset..atlas_offset + 4], &[7; 4]);
    }

    #[test]
    fn animated_pbr_companions_share_the_base_frame_upload() {
        let mut atlas = animation_atlas(8, 12);
        let frame_len = 8 * 12 * 4;
        let animation = atlas.animated.get_mut(&7).unwrap();
        animation.normal_frames = Some(vec![vec![3; frame_len], vec![4; frame_len]]);
        animation.specular_frames = Some(vec![vec![5; frame_len], vec![6; frame_len]]);
        let mut bytes = Vec::new();
        let mut uploads = Vec::new();

        atlas.animate_tick_into(&mut bytes, &mut uploads);
        atlas.animate_tick_into(&mut bytes, &mut uploads);

        let padded_frame_len = 10 * 14 * 4;
        assert_eq!(bytes.len(), padded_frame_len * 3);
        assert_eq!(uploads[0].buffer_offset, 0);
        assert_eq!(
            uploads[0].normal_buffer_offset,
            Some(padded_frame_len as u64)
        );
        assert_eq!(
            uploads[0].specular_buffer_offset,
            Some((padded_frame_len * 2) as u64)
        );
        let region = atlas.regions[7];
        let atlas_offset = ((region.y * atlas.width + region.x) * 4) as usize;
        assert_eq!(
            &atlas.normal_pixels[atlas_offset..atlas_offset + 4],
            &[4; 4]
        );
        assert_eq!(
            &atlas.specular_pixels[atlas_offset..atlas_offset + 4],
            &[6; 4]
        );
    }

    #[test]
    fn packer_keeps_high_resolution_pixels_and_distinct_uv_regions() {
        let textures = vec![
            PendingTexture {
                index: 0,
                pixels: vec![1; 32 * 32 * 4],
                width: 32,
                height: 32,
            },
            PendingTexture {
                index: MISSING_TILE_INDEX,
                pixels: vec![2; 16 * 16 * 4],
                width: 16,
                height: 16,
            },
        ];
        let (width, height, regions) = pack_regions(&textures);
        assert_eq!(regions[0].width, 32);
        assert_eq!(regions[0].height, 32);
        assert_ne!(regions[0], regions[MISSING_TILE_INDEX]);
        let mut pixels = vec![0; (width * height * 4) as usize];
        copy_sprite_with_gutter(&mut pixels, width, regions[0], &textures[0].pixels);
        let final_pixel = (((regions[0].y + 31) * width + regions[0].x + 31) * 4) as usize;
        assert_eq!(&pixels[final_pixel..final_pixel + 4], &[1; 4]);
    }

    #[test]
    fn pbr_companion_atlases_share_regions_and_use_iris_fallbacks() {
        let textures = vec![
            PendingTexture {
                index: 0,
                pixels: vec![1; 2 * 2 * 4],
                width: 2,
                height: 2,
            },
            PendingTexture {
                index: 1,
                pixels: vec![2; 2 * 2 * 4],
                width: 2,
                height: 2,
            },
        ];
        let (width, height, regions) = pack_regions(&textures);
        let normal_map = vec![9; 2 * 2 * 4];
        let specular_map = vec![6; 2 * 2 * 4];
        let normal = build_companion_atlas(
            width,
            height,
            &textures,
            &regions,
            &HashMap::from([(0, normal_map)]),
            [127, 127, 255, 255],
        );
        let specular = build_companion_atlas(
            width,
            height,
            &textures,
            &regions,
            &HashMap::from([(1, specular_map)]),
            [0, 0, 0, 0],
        );

        fn pixel(atlas: &[u8], region: AtlasRegion, width: u32) -> &[u8] {
            let offset = ((region.y * width + region.x) * 4) as usize;
            &atlas[offset..offset + 4]
        }
        assert_eq!(pixel(&normal, regions[0], width), &[9; 4]);
        assert_eq!(pixel(&normal, regions[1], width), &[127, 127, 255, 255]);
        assert_eq!(pixel(&specular, regions[0], width), &[0, 0, 0, 0]);
        assert_eq!(pixel(&specular, regions[1], width), &[6; 4]);
    }

    #[test]
    fn pbr_companion_filenames_are_not_independent_color_sprites() {
        assert!(is_pbr_companion_filename("stone_n.png"));
        assert!(is_pbr_companion_filename("stone_s.png"));
        assert!(!is_pbr_companion_filename("stone.png"));
        assert!(!is_pbr_companion_filename("nether_star.png"));
    }

    #[test]
    fn uv_region_keeps_the_full_vanilla_texture_extent() {
        let region = AtlasRegion {
            x: 17,
            y: 33,
            width: 16,
            height: 16,
        };
        let uv = region.uv_rect(256, 256);

        assert_eq!(uv[2] - uv[0], 16.0 / 256.0);
        assert_eq!(uv[3] - uv[1], 16.0 / 256.0);
    }

    #[test]
    fn chest_face_crop_scales_with_resource_pack_resolution() {
        let mut source = RgbaImage::new(128, 128);
        source.put_pixel(56, 38, image::Rgba([9, 8, 7, 255]));
        let face = extract_scaled_chest_face(&DynamicImage::ImageRgba8(source), 28, 19, 14, 14);
        assert_eq!(face.dimensions(), (32, 32));
        assert_eq!(face.get_pixel(0, 0).0, [9, 8, 7, 255]);
    }
}
