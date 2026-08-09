//! Vulkan shader compilation and built-in shader assets.
//!
//! The renderer core only stores [`renderer::rhi::types::ShaderCode`]. This
//! module is the Vulkan boundary that turns GLSL source into SPIR-V and owns
//! the default RustCraft shader sources. Shader-pack discovery and source
//! adaptation can therefore stay backend-neutral in the game crate, while the
//! native compiler remains replaceable with another backend implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use renderer::rhi::types::{RhiError, RhiResult};
use renderer::rhi::types::{
    ShaderBinding, ShaderBindingType, ShaderCode, ShaderDesc, ShaderInterface, ShaderLanguage,
    ShaderSource, ShaderStage,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinShader {
    BasicVertex,
    BasicFragment,
    EntityVertex,
    EntityFragment,
    GuiVertex,
    GuiFragment,
    PanoramaVertex,
    PanoramaFragment,
    SkyVertex,
    SkyFragment,
    ToneMappingVertex,
    ToneMappingFragment,
}

impl BuiltinShader {
    pub const ALL: [Self; 12] = [
        Self::BasicVertex,
        Self::BasicFragment,
        Self::EntityVertex,
        Self::EntityFragment,
        Self::GuiVertex,
        Self::GuiFragment,
        Self::PanoramaVertex,
        Self::PanoramaFragment,
        Self::SkyVertex,
        Self::SkyFragment,
        Self::ToneMappingVertex,
        Self::ToneMappingFragment,
    ];

    pub const fn stage(self) -> ShaderStage {
        match self {
            Self::BasicVertex
            | Self::EntityVertex
            | Self::GuiVertex
            | Self::PanoramaVertex
            | Self::SkyVertex
            | Self::ToneMappingVertex => ShaderStage::Vertex,
            Self::BasicFragment
            | Self::EntityFragment
            | Self::GuiFragment
            | Self::PanoramaFragment
            | Self::SkyFragment
            | Self::ToneMappingFragment => ShaderStage::Fragment,
        }
    }

    pub const fn source_name(self) -> &'static str {
        match self {
            Self::BasicVertex => "builtin/basic.vert",
            Self::BasicFragment => "builtin/basic.frag",
            Self::EntityVertex => "builtin/entity.vert",
            Self::EntityFragment => "builtin/entity.frag",
            Self::GuiVertex => "builtin/gui.vert",
            Self::GuiFragment => "builtin/gui.frag",
            Self::PanoramaVertex => "builtin/panorama.vert",
            Self::PanoramaFragment => "builtin/panorama.frag",
            Self::SkyVertex => "builtin/sky.vert",
            Self::SkyFragment => "builtin/sky.frag",
            Self::ToneMappingVertex => "builtin/tone_mapping.vert",
            Self::ToneMappingFragment => "builtin/tone_mapping.frag",
        }
    }

    pub fn source(self) -> ShaderSource {
        ShaderSource::glsl(builtin_source(self), self.source_name())
    }

    pub fn descriptor(self) -> ShaderDesc {
        let mut desc = ShaderDesc::new(self.stage(), ShaderCode::Source(self.source()))
            .with_interface(self.interface());
        desc.label = self.source_name().to_string();
        desc
    }

    /// Declare the shader-visible resources used by the built-in module.
    ///
    /// The declaration is intentionally expressed in RHI terms. Vulkan turns
    /// it into descriptor set layouts, while another backend can map the same
    /// interface to its own resource binding model.
    pub fn interface(self) -> ShaderInterface {
        let stage = self.stage().mask();
        let mut interface = ShaderInterface::default();
        match self {
            Self::BasicVertex => {
                interface = interface
                    .with_binding(ShaderBinding::new(
                        "Uniforms",
                        0,
                        0,
                        ShaderBindingType::UniformBuffer,
                        stage,
                    ))
                    .with_push_constants(16);
            }
            Self::BasicFragment | Self::EntityFragment => {
                interface = interface
                    .with_binding(ShaderBinding::new(
                        "Uniforms",
                        0,
                        0,
                        ShaderBindingType::UniformBuffer,
                        stage,
                    ))
                    .with_binding(ShaderBinding::new(
                        "tex_sampler",
                        0,
                        1,
                        ShaderBindingType::CombinedImageSampler,
                        stage,
                    ));
            }
            Self::EntityVertex => {
                interface = interface.with_binding(ShaderBinding::new(
                    "Uniforms",
                    0,
                    0,
                    ShaderBindingType::UniformBuffer,
                    stage,
                ));
            }
            Self::GuiVertex => {
                interface = interface.with_binding(ShaderBinding::new(
                    "GuiUniforms",
                    0,
                    0,
                    ShaderBindingType::UniformBuffer,
                    stage,
                ));
            }
            Self::GuiFragment => {
                interface = interface.with_binding(ShaderBinding::new(
                    "gui_tex",
                    0,
                    1,
                    ShaderBindingType::CombinedImageSampler,
                    stage,
                ));
            }
            Self::PanoramaVertex => {}
            Self::PanoramaFragment => {
                interface = interface
                    .with_binding(ShaderBinding::new(
                        "PanoramaUniforms",
                        0,
                        0,
                        ShaderBindingType::UniformBuffer,
                        stage,
                    ))
                    .with_binding(ShaderBinding::new(
                        "tex_panorama",
                        0,
                        1,
                        ShaderBindingType::CombinedImageSampler,
                        stage,
                    ));
            }
            Self::SkyVertex => {
                interface = interface.with_binding(ShaderBinding::new(
                    "SkyUniforms",
                    0,
                    0,
                    ShaderBindingType::UniformBuffer,
                    stage,
                ));
            }
            Self::SkyFragment => {
                interface = interface
                    .with_binding(ShaderBinding::new(
                        "SkyUniforms",
                        0,
                        0,
                        ShaderBindingType::UniformBuffer,
                        stage,
                    ))
                    .with_binding(ShaderBinding::new(
                        "sun_tex",
                        0,
                        1,
                        ShaderBindingType::CombinedImageSampler,
                        stage,
                    ))
                    .with_binding(ShaderBinding::new(
                        "moon_tex",
                        0,
                        2,
                        ShaderBindingType::CombinedImageSampler,
                        stage,
                    ))
                    .with_binding(ShaderBinding::new(
                        "custom_sky_tex",
                        0,
                        3,
                        ShaderBindingType::CombinedImageSampler,
                        stage,
                    ));
            }
            Self::ToneMappingVertex => {}
            Self::ToneMappingFragment => {
                interface = interface.with_binding(ShaderBinding::new(
                    "tone_source",
                    0,
                    0,
                    ShaderBindingType::CombinedImageSampler,
                    stage,
                ));
            }
        }
        interface
    }
}

pub fn compile_source_to_spirv(
    source: &ShaderSource,
    stage: ShaderStage,
    entry_point: &str,
) -> RhiResult<Vec<u8>> {
    if source.language != ShaderLanguage::Glsl {
        return Err(RhiError::Backend(format!(
            "Vulkan shader compiler does not support {:?} source",
            source.language
        )));
    }
    if source.source.trim().is_empty() {
        return Err(RhiError::InvalidDescription(
            "shader source must be non-empty",
        ));
    }

    let compiler = shaderc::Compiler::new()
        .map_err(|error| RhiError::Backend(format!("failed to create shader compiler: {error}")))?;
    let mut options = shaderc::CompileOptions::new()
        .map_err(|error| RhiError::Backend(format!("create shader options: {error}")))?;
    options.set_target_env(
        shaderc::TargetEnv::Vulkan,
        shaderc::EnvVersion::Vulkan1_3 as u32,
    );
    options.set_source_language(shaderc::SourceLanguage::GLSL);
    options.set_auto_map_locations(true);

    for define in &source.defines {
        options.add_macro_definition(&define.name, define.value.as_deref());
    }

    let includes = source
        .includes
        .iter()
        .map(|include| (include.name.clone(), include.source.clone()))
        .collect::<HashMap<_, _>>();
    options.set_include_callback(move |requested, include_type, requesting, _depth| {
        let resolved_name = resolve_include_name(requested, include_type, requesting, &includes)
            .ok_or_else(|| format!("include '{requested}' does not exist"))?;
        let content = includes
            .get(&resolved_name)
            .cloned()
            .ok_or_else(|| format!("include '{resolved_name}' does not exist"))?;
        Ok(shaderc::ResolvedInclude {
            resolved_name,
            content,
        })
    });

    let artifact = compiler
        .compile_into_spirv(
            &source.source,
            shader_kind(stage)?,
            &source.source_name,
            entry_point,
            Some(&options),
        )
        .map_err(|error| {
            RhiError::Backend(format!("compile shader '{}': {error}", source.source_name))
        })?;
    Ok(artifact.as_binary_u8().to_vec())
}

fn shader_kind(stage: ShaderStage) -> RhiResult<shaderc::ShaderKind> {
    Ok(match stage {
        ShaderStage::Vertex => shaderc::ShaderKind::Vertex,
        ShaderStage::Fragment => shaderc::ShaderKind::Fragment,
        ShaderStage::Geometry => shaderc::ShaderKind::Geometry,
        ShaderStage::TessellationControl => shaderc::ShaderKind::TessControl,
        ShaderStage::TessellationEvaluation => shaderc::ShaderKind::TessEvaluation,
        ShaderStage::Compute => shaderc::ShaderKind::Compute,
    })
}

fn resolve_include_name(
    requested: &str,
    include_type: shaderc::IncludeType,
    requesting: &str,
    includes: &HashMap<String, String>,
) -> Option<String> {
    let requested = requested.replace('\\', "/");
    let exact = requested.trim_start_matches('/').to_string();
    if includes.contains_key(&exact) {
        return Some(exact);
    }

    if include_type == shaderc::IncludeType::Relative {
        let parent = Path::new(requesting)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let candidate = normalize_path(&parent.join(&requested))?;
        if includes.contains_key(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn normalize_path(path: &Path) -> Option<String> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(component) => result.push(component),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(result.to_string_lossy().replace('\\', "/"))
}

fn builtin_source(shader: BuiltinShader) -> &'static str {
    match shader {
        BuiltinShader::BasicVertex => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/basic.vert"))
        }
        BuiltinShader::BasicFragment => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/basic.frag"))
        }
        BuiltinShader::EntityVertex => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/entity.vert"))
        }
        BuiltinShader::EntityFragment => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/entity.frag"))
        }
        BuiltinShader::GuiVertex => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/gui.vert"))
        }
        BuiltinShader::GuiFragment => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/gui.frag"))
        }
        BuiltinShader::PanoramaVertex => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/panorama.vert"
            ))
        }
        BuiltinShader::PanoramaFragment => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/shaders/panorama.frag"
            ))
        }
        BuiltinShader::SkyVertex => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/sky.vert"))
        }
        BuiltinShader::SkyFragment => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/sky.frag"))
        }
        BuiltinShader::ToneMappingVertex => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/tone_mapping.vert"
        )),
        BuiltinShader::ToneMappingFragment => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/tone_mapping.frag"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_descriptors_are_backend_owned_sources() {
        let desc = BuiltinShader::GuiVertex.descriptor();
        assert!(matches!(
            desc.code,
            renderer::rhi::types::ShaderCode::Source(_)
        ));
        assert_eq!(desc.stage, ShaderStage::Vertex);
    }

    #[test]
    fn builtin_glsl_compiles_to_spirv() {
        let source = BuiltinShader::GuiVertex.source();
        let spirv = compile_source_to_spirv(&source, ShaderStage::Vertex, "main").unwrap();
        assert_eq!(spirv.get(0..4), Some(&[0x03, 0x02, 0x23, 0x07][..]));
    }

    #[test]
    fn every_builtin_shader_compiles_to_spirv() {
        for shader in BuiltinShader::ALL {
            let source = shader.source();
            let spirv = compile_source_to_spirv(&source, shader.stage(), "main")
                .unwrap_or_else(|error| panic!("{} failed: {error}", shader.source_name()));
            assert_eq!(
                spirv.get(0..4),
                Some(&[0x03, 0x02, 0x23, 0x07][..]),
                "{} did not produce SPIR-V",
                shader.source_name()
            );
        }
    }
}
