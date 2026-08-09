//! Shader-pack discovery and runtime GLSL compilation inputs.
//!
//! Packs live in `shaderpacks/` as either directories or zip files. Every pack
//! can use the RustCraft manifest or the conventional OptiFine/Iris layout.

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path};

use renderer_api::rhi::types::{
    ShaderCode, ShaderDefine, ShaderInclude, ShaderLanguage, ShaderSource,
};

mod optifine;
pub use optifine::{
    OptifineCompileReport, OptifineCompiledStage, OptifineDiagnostic, OptifineDiagnosticCode,
    OptifineDiagnosticSeverity, OptifineManifest, OptifineOption, OptifineOptionKind,
    OptifineOptionSource, OptifinePass, OptifineProfile, OptifineProfileAction, OptifineProgram,
    OptifineProgramActivation, OptifineScreen, OptifineScreenEntry, OptifineShaderPack,
    OptifineStage, OptifineStageSource,
};
mod glsl_patcher;
pub mod std140;

pub const SHADER_PACK_DIR: &str = "shaderpacks";
pub const MANIFEST_NAME: &str = "rustcraft-shaders.json";
pub const SHADER_PACK_FORMAT: u32 = 1;
const MAX_PACK_FILES: usize = 4096;
const MAX_SHADER_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderCapabilities {
    pub ray_tracing: bool,
    pub fsr3: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderPackFormat {
    RustCraft,
    OptifineIris,
}

impl ShaderPackFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::RustCraft => "RustCraft Vulkan",
            Self::OptifineIris => "OptiFine/Iris",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderPackInfo {
    pub source_name: String,
    pub name: String,
    pub description: String,
    pub format: ShaderPackFormat,
    pub program_count: usize,
    pub option_count: usize,
    pub compatible: bool,
    pub requires_ray_tracing: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    WorldVertex,
    WorldFragment,
    EntityVertex,
    EntityFragment,
    SkyVertex,
    SkyFragment,
}

impl ShaderStage {
    fn from_manifest_key(key: &str) -> Option<Self> {
        Some(match key {
            "world.vertex" => Self::WorldVertex,
            "world.fragment" => Self::WorldFragment,
            "entity.vertex" => Self::EntityVertex,
            "entity.fragment" => Self::EntityFragment,
            "sky.vertex" => Self::SkyVertex,
            "sky.fragment" => Self::SkyFragment,
            _ => return None,
        })
    }

    pub const fn rhi_stage(self) -> renderer_api::rhi::types::ShaderStage {
        match self {
            Self::WorldVertex | Self::EntityVertex | Self::SkyVertex => {
                renderer_api::rhi::types::ShaderStage::Vertex
            }
            Self::WorldFragment | Self::EntityFragment | Self::SkyFragment => {
                renderer_api::rhi::types::ShaderStage::Fragment
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
struct ShaderPackManifest {
    format_version: u32,
    name: String,
    description: String,
    stages: BTreeMap<String, String>,
    defines: BTreeMap<String, String>,
    requires: ShaderPackRequirements,
}

impl Default for ShaderPackManifest {
    fn default() -> Self {
        Self {
            format_version: 0,
            name: String::new(),
            description: String::new(),
            stages: BTreeMap::new(),
            defines: BTreeMap::new(),
            requires: ShaderPackRequirements::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ShaderPackRequirements {
    ray_tracing: bool,
    fsr3: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ShaderPackShaders {
    pub stages: HashMap<ShaderStage, ShaderCode>,
    pub active_name: Option<String>,
    pub error: Option<String>,
}

impl ShaderPackShaders {
    pub fn load_selected(selected: Option<&str>, capabilities: RenderCapabilities) -> Self {
        let Some(selected) = selected.filter(|name| !name.is_empty()) else {
            return Self::default();
        };
        match load_pack(selected, capabilities) {
            Ok(pack) => pack,
            Err(_rustcraft_error) => {
                // Not a RustCraft pack — try OptiFine/Iris.
                match load_optifine_shader_pack(selected) {
                    Ok(_optifine_pack) => {
                        log::info!(
                            "OptiFine/Iris pack '{}' detected; G-buffer execution will be initialised after swapchain creation",
                            selected
                        );
                        Self {
                            stages: HashMap::new(),
                            active_name: Some(selected.to_string()),
                            error: None,
                        }
                    }
                    Err(optifine_error) => {
                        log::error!(
                            "shader pack '{selected}' could not be loaded as RustCraft ({_rustcraft_error}) nor OptiFine/Iris ({optifine_error})"
                        );
                        Self {
                            stages: HashMap::new(),
                            active_name: None,
                            error: Some(format!(
                                "Not a recognised RustCraft or OptiFine/Iris pack: {_rustcraft_error}"
                            )),
                        }
                    }
                }
            }
        }
    }

    pub fn stage(&self, stage: ShaderStage, fallback: &ShaderCode) -> ShaderCode {
        self.stages
            .get(&stage)
            .cloned()
            .unwrap_or_else(|| fallback.clone())
    }
}

pub fn discover_shader_packs(capabilities: RenderCapabilities) -> Vec<ShaderPackInfo> {
    let root = Path::new(SHADER_PACK_DIR);
    let _ = fs::create_dir_all(root);
    let mut packs = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return packs;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir()
            && !path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            continue;
        }
        let source_name = entry.file_name().to_string_lossy().into_owned();
        packs.push(inspect_pack(&source_name, capabilities));
    }
    packs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    packs
}

fn inspect_pack(source_name: &str, capabilities: RenderCapabilities) -> ShaderPackInfo {
    let fallback_name = source_name.trim_end_matches(".zip").to_string();
    match PackFiles::open(source_name) {
        Ok(files) => match parse_manifest(&files) {
            Ok(manifest) => {
                let format_ok = manifest.format_version == SHADER_PACK_FORMAT;
                let rt_ok = !manifest.requires.ray_tracing || capabilities.ray_tracing;
                let fsr3_ok = !manifest.requires.fsr3 || capabilities.fsr3;
                let error = if !format_ok {
                    Some(format!(
                        "Unsupported format {} (expected {})",
                        manifest.format_version, SHADER_PACK_FORMAT
                    ))
                } else if !rt_ok {
                    Some("Requires VK_KHR_ray_tracing_pipeline".to_string())
                } else if !fsr3_ok {
                    Some("Requires AMD FidelityFX FSR 3".to_string())
                } else {
                    None
                };
                ShaderPackInfo {
                    source_name: source_name.to_string(),
                    name: if manifest.name.is_empty() {
                        fallback_name
                    } else {
                        manifest.name
                    },
                    description: manifest.description,
                    format: ShaderPackFormat::RustCraft,
                    program_count: manifest.stages.len(),
                    option_count: manifest.defines.len(),
                    compatible: error.is_none(),
                    requires_ray_tracing: manifest.requires.ray_tracing,
                    error,
                }
            }
            Err(manifest_error) => inspect_optifine_iris_pack(&files, source_name, &fallback_name)
                .unwrap_or_else(|| ShaderPackInfo {
                    source_name: source_name.to_string(),
                    name: fallback_name,
                    description: String::new(),
                    format: ShaderPackFormat::RustCraft,
                    program_count: 0,
                    option_count: 0,
                    compatible: false,
                    requires_ray_tracing: false,
                    error: Some(manifest_error),
                }),
        },
        Err(error) => ShaderPackInfo {
            source_name: source_name.to_string(),
            name: fallback_name,
            description: String::new(),
            format: ShaderPackFormat::RustCraft,
            program_count: 0,
            option_count: 0,
            compatible: false,
            requires_ray_tracing: false,
            error: Some(error),
        },
    }
}

fn inspect_optifine_iris_pack(
    files: &PackFiles,
    source_name: &str,
    fallback_name: &str,
) -> Option<ShaderPackInfo> {
    let manifest = optifine::manifest_from_files(&files.files)?;
    let program_count = manifest.programs.len();
    let complete_programs = manifest
        .programs
        .iter()
        .filter(|program| program.complete)
        .count();
    Some(ShaderPackInfo {
        source_name: source_name.to_string(),
        name: fallback_name.to_string(),
        description: format!(
            "Detected {program_count} OptiFine/Iris programs ({complete_programs} complete stage sets)"
        ),
        format: ShaderPackFormat::OptifineIris,
        program_count,
        option_count: manifest.options.len(),
        compatible: true,
        requires_ray_tracing: false,
        error: None,
    })
}

pub fn load_optifine_shader_pack(source_name: &str) -> Result<OptifineShaderPack, String> {
    let files = PackFiles::open(source_name)?;
    OptifineShaderPack::from_files(files.files)
        .ok_or_else(|| "pack does not contain an OptiFine/Iris shader layout".to_string())
}

/// Apply the compatibility fallback for Iris-only voxel lighting features.
///
/// The OptiFine/Iris source format lets packs expose coloured lighting and
/// world-space reflection options even when they are only implemented by Iris
/// custom images/SSBOs. RustCraft's current executor supports the regular
/// framebuffer/composite path, but not those storage-image and compute passes;
/// compiling the options as-is would make packs such as Complementary render
/// their own full-screen "switch to Iris" error page. Keep the rest of the
/// pack active and turn only the unsupported options off at compile time.
pub fn sanitize_optifine_compatibility_overrides(
    overrides: &BTreeMap<String, String>,
) -> (BTreeMap<String, String>, Option<String>) {
    let mut sanitized = overrides.clone();
    let mut disabled = Vec::new();

    if overrides
        .get("COLORED_LIGHTING")
        .and_then(|value| value.parse::<i32>().ok())
        .is_some_and(|value| value > 0)
    {
        sanitized.insert("COLORED_LIGHTING".to_string(), "0".to_string());
        disabled.push("colored lighting (Iris custom images)");
    }
    if overrides
        .get("WORLD_SPACE_REFLECTIONS")
        .and_then(|value| value.parse::<i32>().ok())
        .is_some_and(|value| value > 0)
    {
        sanitized.insert("WORLD_SPACE_REFLECTIONS".to_string(), "-1".to_string());
        disabled.push("world-space reflections (Iris voxel images)");
    }

    let notice = (!disabled.is_empty()).then(|| {
        format!(
            "RustCraft compatibility: disabled {}",
            disabled.join(" and ")
        )
    });
    (sanitized, notice)
}

fn load_pack(
    source_name: &str,
    capabilities: RenderCapabilities,
) -> Result<ShaderPackShaders, String> {
    let files = PackFiles::open(source_name)?;
    let manifest = parse_manifest(&files)?;
    if manifest.format_version != SHADER_PACK_FORMAT {
        return Err(format!(
            "unsupported shader pack format {} (expected {})",
            manifest.format_version, SHADER_PACK_FORMAT
        ));
    }
    if manifest.requires.ray_tracing && !capabilities.ray_tracing {
        return Err("shader pack requires Vulkan ray tracing support".to_string());
    }
    if manifest.requires.fsr3 && !capabilities.fsr3 {
        return Err("shader pack requires AMD FidelityFX FSR 3".to_string());
    }

    let mut stages = HashMap::new();
    for (key, path) in &manifest.stages {
        let stage = ShaderStage::from_manifest_key(key)
            .ok_or_else(|| format!("unknown shader stage '{key}'"))?;
        let normalized = normalize_pack_path(path)?;
        let bytes = files
            .get(&normalized)
            .ok_or_else(|| format!("shader '{normalized}' does not exist"))?;
        let code = if normalized.ends_with(".spv") {
            ShaderCode::spirv(validate_spirv(bytes, &normalized)?)
        } else {
            shader_source_from_files(&files, &normalized, bytes, &manifest.defines)?
        };
        stages.insert(stage, code);
    }
    log::info!(
        "loaded shader pack '{}' with {} overridden stages",
        manifest.name,
        stages.len()
    );
    Ok(ShaderPackShaders {
        stages,
        active_name: Some(if manifest.name.is_empty() {
            source_name.to_string()
        } else {
            manifest.name
        }),
        error: None,
    })
}

fn shader_source_from_files(
    files: &PackFiles,
    path: &str,
    bytes: &[u8],
    defines: &BTreeMap<String, String>,
) -> Result<ShaderCode, String> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| format!("shader '{path}' is not valid UTF-8"))?;
    let mut shader = ShaderSource::new(ShaderLanguage::Glsl, source, path);
    for (name, value) in defines {
        shader = shader.with_define(ShaderDefine::new(
            name,
            (!value.is_empty()).then_some(value.clone()),
        ));
    }
    for (include_path, include_bytes) in &files.files {
        if include_path == path {
            continue;
        }
        if let Ok(include_source) = std::str::from_utf8(include_bytes) {
            shader = shader.with_include(ShaderInclude::new(include_path, include_source));
        }
    }
    Ok(ShaderCode::Source(shader))
}

fn parse_manifest(files: &PackFiles) -> Result<ShaderPackManifest, String> {
    let bytes = files
        .get(MANIFEST_NAME)
        .ok_or_else(|| format!("missing {MANIFEST_NAME}"))?;
    serde_json::from_slice(bytes).map_err(|error| format!("invalid {MANIFEST_NAME}: {error}"))
}

fn validate_spirv(bytes: &[u8], path: &str) -> Result<Vec<u8>, String> {
    if bytes.len() % 4 != 0 {
        return Err("SPIR-V byte length is not divisible by four".to_string());
    }
    let magic = bytes
        .chunks_exact(4)
        .next()
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()));
    if magic != Some(0x0723_0203) {
        return Err(format!("shader '{path}' is not valid little-endian SPIR-V"));
    }
    Ok(bytes.to_vec())
}

#[derive(Clone)]
struct PackFiles {
    files: HashMap<String, Vec<u8>>,
}

impl PackFiles {
    fn open(source_name: &str) -> Result<Self, String> {
        if Path::new(source_name).components().count() != 1
            || !matches!(
                Path::new(source_name).components().next(),
                Some(Component::Normal(_))
            )
        {
            return Err("shader pack name must not contain a path".to_string());
        }
        let root = Path::new(SHADER_PACK_DIR).join(source_name);
        let mut files = HashMap::new();
        if root.is_dir() {
            collect_directory(&root, &root, &mut files)?;
        } else {
            let data = fs::read(&root)
                .map_err(|error| format!("failed to read {}: {error}", root.display()))?;
            let mut archive = zip::ZipArchive::new(Cursor::new(data))
                .map_err(|error| format!("invalid shader pack zip: {error}"))?;
            if archive.len() > MAX_PACK_FILES {
                return Err(format!(
                    "shader pack contains more than {MAX_PACK_FILES} files"
                ));
            }
            for index in 0..archive.len() {
                let mut entry = archive
                    .by_index(index)
                    .map_err(|error| format!("invalid zip entry: {error}"))?;
                if entry.is_dir() {
                    continue;
                }
                if entry.size() > MAX_SHADER_FILE_BYTES {
                    return Err(format!("shader pack entry '{}' is too large", entry.name()));
                }
                let path = normalize_pack_path(entry.name())?;
                let mut data = Vec::new();
                entry
                    .read_to_end(&mut data)
                    .map_err(|error| format!("failed to read '{path}': {error}"))?;
                files.insert(path, data);
            }
            strip_single_root_directory(&mut files);
        }
        Ok(Self { files })
    }

    fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    output: &mut HashMap<String, Vec<u8>>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_directory(root, &path, output)?;
        } else {
            if output.len() >= MAX_PACK_FILES {
                return Err(format!(
                    "shader pack contains more than {MAX_PACK_FILES} files"
                ));
            }
            let file_size = entry
                .metadata()
                .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?
                .len();
            if file_size > MAX_SHADER_FILE_BYTES {
                return Err(format!(
                    "shader pack file '{}' is too large",
                    path.display()
                ));
            }
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            let normalized = normalize_pack_path(&relative.to_string_lossy())?;
            let data = fs::read(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            output.insert(normalized, data);
        }
    }
    Ok(())
}

fn strip_single_root_directory(files: &mut HashMap<String, Vec<u8>>) {
    if files.contains_key(MANIFEST_NAME)
        || files.contains_key(optifine::PROPERTIES_PATH)
        || files.keys().any(|path| {
            path.starts_with("shaders/")
                && matches!(
                    Path::new(path)
                        .extension()
                        .and_then(|extension| extension.to_str()),
                    Some("vsh" | "fsh" | "gsh" | "csh" | "tcs" | "tes")
                )
        })
    {
        return;
    }
    let Some(prefix) = files
        .keys()
        .find_map(|path| path.split_once('/').map(|(prefix, _)| prefix.to_string()))
    else {
        return;
    };
    let rustcraft_manifest = format!("{prefix}/{MANIFEST_NAME}");
    let optifine_properties = format!("{prefix}/{}", optifine::PROPERTIES_PATH);
    let has_optifine_program = files.keys().any(|path| {
        path.starts_with(&format!("{prefix}/shaders/"))
            && matches!(
                Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str()),
                Some("vsh" | "fsh" | "gsh" | "csh" | "tcs" | "tes")
            )
    });
    if !files.contains_key(&rustcraft_manifest)
        && !files.contains_key(&optifine_properties)
        && !has_optifine_program
    {
        return;
    }
    if files
        .keys()
        .any(|path| !path.starts_with(&format!("{prefix}/")))
    {
        return;
    }
    let old = std::mem::take(files);
    *files = old
        .into_iter()
        .map(|(path, data)| (path[prefix.len() + 1..].to_string(), data))
        .collect();
}

fn normalize_pack_path(path: &str) -> Result<String, String> {
    let path = path.replace('\\', "/");
    let mut parts = Vec::new();
    for component in Path::new(&path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("path escapes the shader pack".to_string());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("absolute paths are not allowed".to_string())
            }
        }
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_cannot_escape_pack() {
        assert_eq!(
            normalize_pack_path("shaders/../common.glsl").unwrap(),
            "common.glsl"
        );
        assert!(normalize_pack_path("../secret").is_err());
        assert!(PackFiles::open("../outside.zip").is_err());
    }

    #[test]
    fn manifest_defaults_optional_fields() {
        let manifest: ShaderPackManifest = serde_json::from_str(
            r#"{"format_version":1,"name":"Test","stages":{"world.vertex":"world.vert"}}"#,
        )
        .unwrap();
        assert_eq!(manifest.format_version, SHADER_PACK_FORMAT);
        assert_eq!(manifest.name, "Test");
        assert!(!manifest.requires.ray_tracing);
        assert!(!manifest.requires.fsr3);
    }

    #[test]
    fn detects_optifine_iris_layouts_without_claiming_compatibility() {
        let files = PackFiles {
            files: HashMap::from([
                ("shaders/shaders.properties".to_string(), b"".to_vec()),
                ("shaders/gbuffers_terrain.vsh".to_string(), b"".to_vec()),
                ("shaders/gbuffers_terrain.fsh".to_string(), b"".to_vec()),
            ]),
        };
        let info = inspect_optifine_iris_pack(&files, "test.zip", "test").unwrap();
        assert_eq!(info.format, ShaderPackFormat::OptifineIris);
        assert_eq!(info.program_count, 1);
        assert!(info.compatible);
        assert!(info.error.is_none());
    }

    #[test]
    fn strips_single_zip_root_for_optifine_layout() {
        let mut files = HashMap::from([
            (
                "PackName/shaders/shaders.properties".to_string(),
                Vec::new(),
            ),
            (
                "PackName/shaders/gbuffers_basic.vsh".to_string(),
                Vec::new(),
            ),
            (
                "PackName/shaders/gbuffers_basic.fsh".to_string(),
                Vec::new(),
            ),
        ]);
        strip_single_root_directory(&mut files);
        assert!(files.contains_key("shaders/shaders.properties"));
        assert!(files.contains_key("shaders/gbuffers_basic.vsh"));
        assert!(!files.keys().any(|path| path.starts_with("PackName/")));
    }

    #[test]
    fn runtime_shader_source_preserves_includes_for_the_backend() {
        let files = PackFiles {
            files: HashMap::from([
                (
                    "shaders/main.vert".to_string(),
                    br#"#version 450
#include "common.glsl"
void main() { gl_Position = make_position(); }
"#
                    .to_vec(),
                ),
                (
                    "shaders/common.glsl".to_string(),
                    b"vec4 make_position() { return vec4(0.0); }\n".to_vec(),
                ),
            ]),
        };
        let code = shader_source_from_files(
            &files,
            "shaders/main.vert",
            files.get("shaders/main.vert").unwrap(),
            &BTreeMap::new(),
        )
        .unwrap();
        let ShaderCode::Source(source) = code else {
            panic!("GLSL source must stay source data until the backend compiles it");
        };
        assert_eq!(source.language, ShaderLanguage::Glsl);
        assert_eq!(source.includes[0].name, "shaders/common.glsl");
    }
}
