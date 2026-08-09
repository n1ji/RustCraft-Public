//! Backend-neutral shader contracts.
//!
//! This module intentionally stops at shader assets and their declared
//! interface. Source translation (GLSL/HLSL/WGSL to a native module) belongs
//! to the selected backend, while pack loaders such as OptiFine/Iris can keep
//! their source and compatibility logic in the game crate.

use super::types::{ShaderHandle, ShaderStages};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Geometry,
    TessellationControl,
    TessellationEvaluation,
    Compute,
}

impl ShaderStage {
    pub const fn mask(self) -> ShaderStages {
        match self {
            Self::Vertex => ShaderStages::VERTEX,
            Self::Fragment => ShaderStages::FRAGMENT,
            Self::Geometry => ShaderStages::GEOMETRY,
            Self::TessellationControl => ShaderStages::TESSELLATION_CONTROL,
            Self::TessellationEvaluation => ShaderStages::TESSELLATION_EVALUATION,
            Self::Compute => ShaderStages::COMPUTE,
        }
    }
}

/// Source languages accepted by the RHI contract.
///
/// A backend is free to support only a subset. For example, the Vulkan
/// backend currently compiles GLSL through shaderc, while a future backend
/// may consume WGSL directly or translate HLSL through its own toolchain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderLanguage {
    Glsl,
    Hlsl,
    Wgsl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderSourceOrigin {
    Builtin,
    RustCraftPack,
    Optifine,
    Iris,
    User,
}

impl Default for ShaderSourceOrigin {
    fn default() -> Self {
        Self::User
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderBinaryFormat {
    SpirV,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderDefine {
    pub name: String,
    pub value: Option<String>,
}

impl ShaderDefine {
    pub fn new(name: impl Into<String>, value: Option<impl Into<String>>) -> Self {
        Self {
            name: name.into(),
            value: value.map(Into::into),
        }
    }

    pub fn flag(name: impl Into<String>) -> Self {
        Self::new(name, Option::<String>::None)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderInclude {
    pub name: String,
    pub source: String,
}

impl ShaderInclude {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
        }
    }
}

/// Backend-neutral shader source. Includes, defines, and origin remain part of
/// the resource so a backend can compile a pack without the game knowing its
/// native compiler or descriptor implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderSource {
    pub language: ShaderLanguage,
    pub source: String,
    pub source_name: String,
    pub origin: ShaderSourceOrigin,
    pub defines: Vec<ShaderDefine>,
    pub includes: Vec<ShaderInclude>,
}

impl ShaderSource {
    pub fn new(
        language: ShaderLanguage,
        source: impl Into<String>,
        source_name: impl Into<String>,
    ) -> Self {
        Self {
            language,
            source: source.into(),
            source_name: source_name.into(),
            origin: ShaderSourceOrigin::User,
            defines: Vec::new(),
            includes: Vec::new(),
        }
    }

    pub fn glsl(source: impl Into<String>, source_name: impl Into<String>) -> Self {
        Self::new(ShaderLanguage::Glsl, source, source_name)
    }

    pub fn with_origin(mut self, origin: ShaderSourceOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_define(mut self, define: ShaderDefine) -> Self {
        self.defines.push(define);
        self
    }

    pub fn with_include(mut self, include: ShaderInclude) -> Self {
        self.includes.push(include);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.source.trim().is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShaderCode {
    Source(ShaderSource),
    Binary {
        format: ShaderBinaryFormat,
        bytes: Vec<u8>,
    },
}

impl ShaderCode {
    pub fn glsl(source: impl Into<String>, source_name: impl Into<String>) -> Self {
        Self::Source(ShaderSource::glsl(source, source_name))
    }

    pub fn spirv(bytes: Vec<u8>) -> Self {
        Self::Binary {
            format: ShaderBinaryFormat::SpirV,
            bytes,
        }
    }

    pub fn source(&self) -> Option<&ShaderSource> {
        match self {
            Self::Source(source) => Some(source),
            Self::Binary { .. } => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Source(source) => source.is_empty(),
            Self::Binary { bytes, .. } => bytes.is_empty(),
        }
    }
}

/// Resource kinds which can be declared by a shader stage.
///
/// The names intentionally describe shader-visible resources rather than
/// Vulkan descriptor types. The backend maps them to its native binding model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderBindingType {
    UniformBuffer,
    StorageBuffer,
    SampledTexture,
    StorageTexture,
    Sampler,
    CombinedImageSampler,
    AccelerationStructure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderBinding {
    pub name: String,
    pub set: u32,
    pub binding: u32,
    pub ty: ShaderBindingType,
    pub array_size: u32,
    pub stages: ShaderStages,
}

impl ShaderBinding {
    pub fn new(
        name: impl Into<String>,
        set: u32,
        binding: u32,
        ty: ShaderBindingType,
        stages: ShaderStages,
    ) -> Self {
        Self {
            name: name.into(),
            set,
            binding,
            ty,
            array_size: 1,
            stages,
        }
    }

    pub fn with_array_size(mut self, array_size: u32) -> Self {
        self.array_size = array_size.max(1);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShaderInterface {
    pub bindings: Vec<ShaderBinding>,
    pub push_constant_size: u32,
}

impl ShaderInterface {
    pub fn with_binding(mut self, binding: ShaderBinding) -> Self {
        self.bindings.push(binding);
        self
    }

    pub fn with_push_constants(mut self, size: u32) -> Self {
        self.push_constant_size = size;
        self
    }

    pub fn binding(&self, set: u32, binding: u32) -> Option<&ShaderBinding> {
        self.bindings
            .iter()
            .find(|entry| entry.set == set && entry.binding == binding)
    }

    pub fn max_set(&self) -> Option<u32> {
        self.bindings.iter().map(|binding| binding.set).max()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderDesc {
    pub stage: ShaderStage,
    pub code: ShaderCode,
    pub entry_point: String,
    pub label: String,
    pub interface: ShaderInterface,
}

impl ShaderDesc {
    pub fn new(stage: ShaderStage, code: ShaderCode) -> Self {
        Self {
            stage,
            code,
            entry_point: String::from("main"),
            label: String::new(),
            interface: ShaderInterface::default(),
        }
    }

    pub fn from_glsl(
        stage: ShaderStage,
        source: impl Into<String>,
        source_name: impl Into<String>,
    ) -> Self {
        Self::new(stage, ShaderCode::glsl(source, source_name))
    }

    pub fn from_spirv(stage: ShaderStage, bytes: Vec<u8>) -> Self {
        Self::new(stage, ShaderCode::spirv(bytes))
    }

    pub fn with_interface(mut self, interface: ShaderInterface) -> Self {
        self.interface = interface;
        self
    }
}

/// A logical shader program is the stage group consumed by a render pass.
/// It deliberately contains handles rather than native modules, so a game
/// shader-pack loader can describe OptiFine/Iris programs before a backend
/// creates pipelines for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderProgramDesc {
    pub name: String,
    pub stages: Vec<ShaderHandle>,
    pub color_targets: Vec<u8>,
    pub writes_depth: bool,
    pub label: String,
}

impl ShaderProgramDesc {
    pub fn new(name: impl Into<String>, stages: Vec<ShaderHandle>) -> Self {
        let name = name.into();
        Self {
            label: name.clone(),
            name,
            stages,
            color_targets: vec![0],
            writes_depth: false,
        }
    }

    pub fn with_color_targets(mut self, color_targets: impl Into<Vec<u8>>) -> Self {
        self.color_targets = color_targets.into();
        self
    }
}
