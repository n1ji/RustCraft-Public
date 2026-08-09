use std::fmt;
use std::ops::{BitOr, BitOrAssign};

macro_rules! define_handle {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name {
            index: u32,
            generation: u32,
        }

        impl $name {
            pub const INVALID: Self = Self {
                index: u32::MAX,
                generation: 0,
            };

            pub const fn new(index: u32, generation: u32) -> Self {
                Self { index, generation }
            }

            pub const fn index(self) -> u32 {
                self.index
            }

            pub const fn generation(self) -> u32 {
                self.generation
            }

            pub const fn is_valid(self) -> bool {
                self.index != u32::MAX && self.generation != 0
            }
        }
    };
}

define_handle!(BufferHandle);
define_handle!(TextureHandle);
define_handle!(ShaderHandle);
define_handle!(PipelineHandle);
define_handle!(SamplerHandle);
define_handle!(BindGroupHandle);
define_handle!(MaterialHandle);
define_handle!(RenderTargetHandle);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

impl Extent2D {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BufferUsage(pub u32);

impl BufferUsage {
    pub const EMPTY: Self = Self(0);
    pub const VERTEX: Self = Self(1 << 0);
    pub const INDEX: Self = Self(1 << 1);
    pub const UNIFORM: Self = Self(1 << 2);
    pub const STORAGE: Self = Self(1 << 3);
    pub const INDIRECT: Self = Self(1 << 4);
    pub const TRANSFER_SRC: Self = Self(1 << 5);
    pub const TRANSFER_DST: Self = Self(1 << 6);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for BufferUsage {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for BufferUsage {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Compatibility spelling used by the first migration batch. New code should
/// prefer `BufferUsage`.
pub type BufferUsages = BufferUsage;

/// Selects where a buffer is intended to live.
///
/// Upload buffers are CPU writable and are useful for data that changes every
/// frame. Device-local buffers are optimized for GPU reads; backends stage
/// their initial data through a temporary upload resource.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BufferMemory {
    #[default]
    Upload,
    DeviceLocal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureUsage(pub u32);

impl TextureUsage {
    pub const EMPTY: Self = Self(0);
    pub const SAMPLED: Self = Self(1 << 0);
    pub const STORAGE: Self = Self(1 << 1);
    pub const COLOR_ATTACHMENT: Self = Self(1 << 2);
    pub const DEPTH_ATTACHMENT: Self = Self(1 << 3);
    pub const TRANSFER_SRC: Self = Self(1 << 4);
    pub const TRANSFER_DST: Self = Self(1 << 5);
    pub const PRESENT: Self = Self(1 << 6);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for TextureUsage {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for TextureUsage {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Compatibility spelling used by the first migration batch. New code should
/// prefer `TextureUsage`.
pub type TextureUsages = TextureUsage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShaderStages(pub u32);

impl ShaderStages {
    pub const EMPTY: Self = Self(0);
    pub const VERTEX: Self = Self(1 << 0);
    pub const FRAGMENT: Self = Self(1 << 1);
    pub const GEOMETRY: Self = Self(1 << 2);
    pub const TESSELLATION_CONTROL: Self = Self(1 << 3);
    pub const TESSELLATION_EVALUATION: Self = Self(1 << 4);
    pub const COMPUTE: Self = Self(1 << 5);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for ShaderStages {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ShaderStages {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    R8Unorm,
    R8G8B8A8Unorm,
    Rgba8Unorm,
    R8G8B8A8Srgb,
    Rgba8Srgb,
    B8G8R8A8Srgb,
    R16G16Float,
    R16Float,
    R16G16B16A16Float,
    Rgba16Float,
    D24UnormS8Uint,
    D32Float,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexStepMode {
    Vertex,
    Instance,
}

impl Default for VertexStepMode {
    fn default() -> Self {
        Self::Vertex
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    Uint32x2,
    Uint32x3,
    Uint32x4,
    Uint8x4Norm,
}

impl VertexFormat {
    pub const fn byte_size(self) -> u32 {
        match self {
            Self::Float32 | Self::Uint32 => 4,
            Self::Float32x2 | Self::Uint32x2 => 8,
            Self::Float32x3 | Self::Uint32x3 => 12,
            Self::Float32x4 | Self::Uint32x4 => 16,
            Self::Uint8x4Norm => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VertexAttribute {
    pub location: u32,
    pub binding: u32,
    pub format: VertexFormat,
    pub offset: u32,
}

impl VertexAttribute {
    pub const fn new(location: u32, binding: u32, format: VertexFormat, offset: u32) -> Self {
        Self {
            location,
            binding,
            format,
            offset,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VertexBufferLayout {
    pub binding: u32,
    pub stride: u32,
    pub step_mode: VertexStepMode,
    pub attributes: Vec<VertexAttribute>,
}

impl VertexBufferLayout {
    pub fn new(binding: u32, stride: u32, attributes: Vec<VertexAttribute>) -> Self {
        Self {
            binding,
            stride,
            step_mode: VertexStepMode::Vertex,
            attributes,
        }
    }

    pub fn with_step_mode(mut self, step_mode: VertexStepMode) -> Self {
        self.step_mode = step_mode;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
    PatchList,
}

impl Default for PrimitiveTopology {
    fn default() -> Self {
        Self::TriangleList
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CullMode {
    #[default]
    None,
    Front,
    Back,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrontFace {
    #[default]
    CounterClockwise,
    Clockwise,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompareFunction {
    Never,
    Less,
    Equal,
    #[default]
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    Zero,
    #[default]
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturated,
    ConstantColor,
    OneMinusConstantColor,
    ConstantAlpha,
    OneMinusConstantAlpha,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendOperation {
    #[default]
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlendComponent {
    pub src_factor: BlendFactor,
    pub dst_factor: BlendFactor,
    pub operation: BlendOperation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlendState {
    pub color: BlendComponent,
    pub alpha: BlendComponent,
}

impl BlendState {
    pub const fn alpha_blend() -> Self {
        Self {
            color: BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
        }
    }
}

pub use super::shader::{
    ShaderBinaryFormat, ShaderBinding, ShaderBindingType, ShaderCode, ShaderDefine, ShaderDesc,
    ShaderInclude, ShaderInterface, ShaderLanguage, ShaderProgramDesc, ShaderSource,
    ShaderSourceOrigin, ShaderStage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferDesc {
    pub size: u64,
    pub usage: BufferUsage,
    pub memory: BufferMemory,
    pub label: String,
}

impl BufferDesc {
    pub fn new(size: u64, usage: BufferUsage, label: impl Into<String>) -> Self {
        Self {
            size,
            usage,
            memory: BufferMemory::Upload,
            label: label.into(),
        }
    }

    pub fn unlabeled(size: u64, usage: BufferUsage) -> Self {
        Self::new(size, usage, "buffer")
    }

    pub fn with_memory(mut self, memory: BufferMemory) -> Self {
        self.memory = memory;
        self
    }

    pub fn device_local(self) -> Self {
        self.with_memory(BufferMemory::DeviceLocal)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureDesc {
    pub extent: Extent2D,
    pub format: TextureFormat,
    pub usage: TextureUsage,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceConfig {
    pub extent: Extent2D,
    pub format: TextureFormat,
    pub vsync: bool,
}

impl SurfaceConfig {
    pub fn new(extent: Extent2D, format: TextureFormat) -> Self {
        Self {
            extent,
            format,
            vsync: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceFrame {
    pub texture: TextureHandle,
    pub image_index: u32,
}

impl TextureDesc {
    pub fn new(extent: Extent2D, format: TextureFormat, usage: TextureUsage) -> Self {
        Self {
            extent,
            format,
            usage,
            mip_levels: 1,
            array_layers: 1,
            label: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineDesc {
    pub vertex_shader: Option<ShaderHandle>,
    pub fragment_shader: Option<ShaderHandle>,
    pub geometry_shader: Option<ShaderHandle>,
    pub tessellation_control_shader: Option<ShaderHandle>,
    pub tessellation_evaluation_shader: Option<ShaderHandle>,
    pub compute_shader: Option<ShaderHandle>,
    pub vertex_buffers: Vec<VertexBufferLayout>,
    pub color_formats: Vec<TextureFormat>,
    pub depth_format: Option<TextureFormat>,
    pub primitive_topology: PrimitiveTopology,
    pub cull_mode: CullMode,
    pub front_face: FrontFace,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_compare: CompareFunction,
    pub blend: Option<BlendState>,
    pub label: String,
}

impl Default for PipelineDesc {
    fn default() -> Self {
        Self {
            vertex_shader: None,
            fragment_shader: None,
            geometry_shader: None,
            tessellation_control_shader: None,
            tessellation_evaluation_shader: None,
            compute_shader: None,
            vertex_buffers: Vec::new(),
            color_formats: Vec::new(),
            depth_format: None,
            primitive_topology: PrimitiveTopology::default(),
            cull_mode: CullMode::default(),
            front_face: FrontFace::default(),
            depth_test: true,
            depth_write: true,
            depth_compare: CompareFunction::default(),
            blend: None,
            label: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingResource {
    Buffer {
        buffer: BufferHandle,
        offset: u64,
        size: u64,
    },
    Texture {
        texture: TextureHandle,
    },
    StorageTexture {
        texture: TextureHandle,
    },
    Sampler {
        sampler: SamplerHandle,
    },
    CombinedImageSampler {
        texture: TextureHandle,
        sampler: SamplerHandle,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BindGroupEntry {
    pub binding: u32,
    pub resource: BindingResource,
}

impl BindGroupEntry {
    pub const fn new(binding: u32, resource: BindingResource) -> Self {
        Self { binding, resource }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BindGroupDesc {
    pub pipeline: PipelineHandle,
    pub set: u32,
    pub entries: Vec<BindGroupEntry>,
    pub label: String,
}

impl BindGroupDesc {
    pub fn new(pipeline: PipelineHandle, set: u32, entries: Vec<BindGroupEntry>) -> Self {
        Self {
            pipeline,
            set,
            entries,
            label: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplerDesc {
    pub linear_filter: bool,
    pub repeat_u: bool,
    pub repeat_v: bool,
    pub repeat_w: bool,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderTargetDesc {
    pub colors: Vec<TextureHandle>,
    pub depth: Option<TextureHandle>,
    pub label: String,
}

impl RenderTargetDesc {
    pub fn new(
        colors: Vec<TextureHandle>,
        depth: Option<TextureHandle>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            colors,
            depth,
            label: label.into(),
        }
    }
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            linear_filter: true,
            repeat_u: true,
            repeat_v: true,
            repeat_w: true,
            label: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceHandle {
    Buffer(BufferHandle),
    Texture(TextureHandle),
    Shader(ShaderHandle),
    Pipeline(PipelineHandle),
    Sampler(SamplerHandle),
    BindGroup(BindGroupHandle),
    RenderTarget(RenderTargetHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceState {
    Undefined,
    CopySrc,
    CopyDst,
    VertexBuffer,
    IndexBuffer,
    UniformBuffer,
    StorageRead,
    StorageWrite,
    ShaderRead,
    ColorAttachment,
    DepthAttachment,
    Present,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResourceBarrier {
    pub resource: ResourceHandle,
    pub before: ResourceState,
    pub after: ResourceState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceCapabilities {
    pub supports_compute: bool,
    pub supports_storage_images: bool,
    pub supports_hdr: bool,
    pub supports_present: bool,
    pub ray_tracing: bool,
    pub compute: bool,
    pub multi_draw_indirect: bool,
    pub hdr_render_targets: bool,
    pub fsr3: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SubmissionId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RhiError {
    InvalidHandle,
    InvalidDescription(&'static str),
    ResourceInUse(&'static str),
    Backend(String),
    Message(String),
}

impl fmt::Display for RhiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHandle => write!(formatter, "invalid resource handle"),
            Self::InvalidDescription(message) => {
                write!(formatter, "invalid description: {message}")
            }
            Self::ResourceInUse(message) => {
                write!(formatter, "resource is still in use: {message}")
            }
            Self::Backend(message) | Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RhiError {}

pub type RhiResult<T> = Result<T, RhiError>;
