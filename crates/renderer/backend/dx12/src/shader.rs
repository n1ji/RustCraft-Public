use std::collections::HashMap;
use std::ffi::c_void;

use renderer::rhi::types::*;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::ID3DBlob;

use super::{backend_error, context, DirectX12Backend, PUSH_CONSTANT_REGISTER};

pub(crate) fn compile_shader(
    desc: &ShaderDesc,
    d3d_compile: context::D3DCompileFn,
) -> RhiResult<Vec<u8>> {
    let hlsl = match &desc.code {
        ShaderCode::Source(source) => match source.language {
            ShaderLanguage::Glsl => {
                let spirv = compile_glsl(source, desc.stage, &desc.entry_point)?;
                spirv_to_hlsl(
                    &spirv,
                    desc.stage,
                    &desc.entry_point,
                    desc.interface.push_constant_size,
                )?
            }
            ShaderLanguage::Hlsl => source.source.clone(),
            ShaderLanguage::Wgsl => {
                return Err(RhiError::Backend(String::from(
                    "Direct3D 12 shader compiler does not support WGSL",
                )))
            }
        },
        ShaderCode::Binary { format, bytes } => match format {
            ShaderBinaryFormat::SpirV => spirv_to_hlsl(
                bytes,
                desc.stage,
                &desc.entry_point,
                desc.interface.push_constant_size,
            )?,
        },
    };
    compile_hlsl(
        &hlsl,
        desc.stage,
        &desc.entry_point,
        &desc.label,
        d3d_compile,
    )
}

fn compile_glsl(
    source: &ShaderSource,
    stage: ShaderStage,
    entry_point: &str,
) -> RhiResult<Vec<u8>> {
    if source.source.trim().is_empty() {
        return Err(RhiError::InvalidDescription(
            "shader source must be non-empty",
        ));
    }
    let compiler = shaderc::Compiler::new()
        .map_err(|error| RhiError::Backend(format!("create GLSL compiler: {error}")))?;
    let mut options = shaderc::CompileOptions::new()
        .map_err(|error| RhiError::Backend(format!("create GLSL compiler options: {error}")))?;
    // SPIRV-Cross 0.23.1 accepts SPIR-V through version 1.5, while a
    // Vulkan 1.3 shaderc target can emit SPIR-V 1.6.  D3D12 only needs the
    // portable shader IR here, so target Vulkan 1.2 for cross-compiler
    // compatibility.
    options.set_target_env(
        shaderc::TargetEnv::Vulkan,
        shaderc::EnvVersion::Vulkan1_2 as u32,
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
        let requested = requested.replace('\\', "/");
        let exact = requested.trim_start_matches('/').to_string();
        let resolved_name = if includes.contains_key(&exact) {
            exact
        } else if include_type == shaderc::IncludeType::Relative {
            let parent = std::path::Path::new(requesting)
                .parent()
                .unwrap_or_else(|| std::path::Path::new(""));
            let candidate = parent.join(&requested).to_string_lossy().replace('\\', "/");
            if includes.contains_key(&candidate) {
                candidate
            } else {
                return Err(format!("include '{requested}' does not exist"));
            }
        } else {
            return Err(format!("include '{requested}' does not exist"));
        };
        Ok(shaderc::ResolvedInclude {
            content: includes
                .get(&resolved_name)
                .cloned()
                .ok_or_else(|| format!("include '{resolved_name}' does not exist"))?,
            resolved_name,
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
            RhiError::Backend(format!(
                "compile GLSL shader '{}': {error}",
                source.source_name
            ))
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

fn spirv_to_hlsl(
    bytes: &[u8],
    stage: ShaderStage,
    entry_point: &str,
    push_constant_size: u32,
) -> RhiResult<String> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err(RhiError::InvalidDescription(
            "SPIR-V shader bytes must contain complete words",
        ));
    }
    if push_constant_size > 16 * 4 {
        return Err(RhiError::InvalidDescription(
            "Direct3D 12 push constants are limited to 64 bytes",
        ));
    }
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect::<Vec<_>>();
    let module = spirv_cross::spirv::Module::from_words(&words);
    let mut ast = spirv_cross::spirv::Ast::<spirv_cross::hlsl::Target>::parse(&module)
        .map_err(|error| backend_error("parse SPIR-V for HLSL", error))?;
    let mut options = spirv_cross::hlsl::CompilerOptions::default();
    options.shader_model = spirv_cross::hlsl::ShaderModel::V5_1;
    // The shared shaders use Vulkan's positive-height viewport convention,
    // where NDC Y=-1 is the top of the render target.  D3D12's viewport maps
    // NDC Y=+1 to the top, so invert the vertex output at the translation
    // boundary instead of duplicating every shader with a D3D-specific path.
    options.vertex.invert_y = true;
    options.entry_point = Some((entry_point.to_string(), execution_model(stage)));
    ast.set_compiler_options(&options)
        .map_err(|error| backend_error("configure SPIR-V HLSL compiler", error))?;
    if push_constant_size > 0 {
        let words = push_constant_size.div_ceil(4);
        ast.set_root_constant_layout(vec![spirv_cross::hlsl::RootConstant {
            start: 0,
            end: words,
            binding: PUSH_CONSTANT_REGISTER,
            space: 0,
        }])
        .map_err(|error| backend_error("configure HLSL push constants", error))?;
    }
    ast.compile()
        .map_err(|error| backend_error("compile SPIR-V to HLSL", error))
}

fn execution_model(stage: ShaderStage) -> spirv_cross::spirv::ExecutionModel {
    match stage {
        ShaderStage::Vertex => spirv_cross::spirv::ExecutionModel::Vertex,
        ShaderStage::Fragment => spirv_cross::spirv::ExecutionModel::Fragment,
        ShaderStage::Geometry => spirv_cross::spirv::ExecutionModel::Geometry,
        ShaderStage::TessellationControl => spirv_cross::spirv::ExecutionModel::TessellationControl,
        ShaderStage::TessellationEvaluation => {
            spirv_cross::spirv::ExecutionModel::TessellationEvaluation
        }
        ShaderStage::Compute => spirv_cross::spirv::ExecutionModel::GlCompute,
    }
}

fn compile_hlsl(
    source: &str,
    stage: ShaderStage,
    entry_point: &str,
    source_name: &str,
    d3d_compile: context::D3DCompileFn,
) -> RhiResult<Vec<u8>> {
    let entry = std::ffi::CString::new(entry_point)
        .map_err(|_| RhiError::InvalidDescription("shader entry point contains nul"))?;
    let target = std::ffi::CString::new(shader_target(stage))
        .map_err(|_| RhiError::InvalidDescription("shader target contains nul"))?;
    let source_name = std::ffi::CString::new(source_name)
        .map_err(|_| RhiError::InvalidDescription("shader name contains nul"))?;
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    unsafe {
        let result = d3d_compile(
            source.as_ptr().cast::<c_void>(),
            source.len(),
            PCSTR(source_name.as_ptr().cast()),
            std::ptr::null(),
            std::ptr::null_mut(),
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            (&mut code as *mut Option<ID3DBlob>) as *mut *mut c_void,
            (&mut errors as *mut Option<ID3DBlob>) as *mut *mut c_void,
        );
        if result.is_err() {
            let message = errors
                .as_ref()
                .map(|blob| blob_message(blob))
                .unwrap_or_else(|| windows::core::Error::from_hresult(result).to_string());
            return Err(backend_error("compile HLSL shader", message));
        }
    }
    let code =
        code.ok_or_else(|| RhiError::Backend(String::from("HLSL compiler returned no bytecode")))?;
    let bytes = unsafe {
        std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize())
    };
    Ok(bytes.to_vec())
}

fn shader_target(stage: ShaderStage) -> &'static str {
    match stage {
        ShaderStage::Vertex => "vs_5_1",
        ShaderStage::Fragment => "ps_5_1",
        ShaderStage::Geometry => "gs_5_1",
        ShaderStage::TessellationControl => "hs_5_1",
        ShaderStage::TessellationEvaluation => "ds_5_1",
        ShaderStage::Compute => "cs_5_1",
    }
}

unsafe fn blob_message(blob: &ID3DBlob) -> String {
    let bytes =
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

pub(crate) fn create_native_shader(
    backend: &DirectX12Backend,
    desc: &ShaderDesc,
) -> RhiResult<context::NativeShader> {
    let d3d_compile = backend.native_ref()?.api.d3d_compile;
    let bytecode = compile_shader(desc, d3d_compile)?;
    Ok(context::NativeShader {
        bytecode,
        stage: desc.stage,
        interface: desc.interface.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glsl_spirv_round_trips_through_hlsl_translation() {
        let source = ShaderSource::glsl(
            "#version 450\nlayout(location = 0) in vec3 position;\nvoid main() { gl_Position = vec4(position, 1.0); }",
            "dx12-test.vert",
        );
        let spirv = compile_glsl(&source, ShaderStage::Vertex, "main")
            .expect("GLSL should compile to SPIR-V");
        let hlsl = spirv_to_hlsl(&spirv, ShaderStage::Vertex, "main", 0)
            .expect("SPIR-V should translate to HLSL");
        assert!(!hlsl.is_empty());
    }
}
