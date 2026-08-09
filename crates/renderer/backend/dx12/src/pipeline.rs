use std::mem::ManuallyDrop;

use renderer::rhi::types::*;
use windows::core::{s, BOOL};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use super::context::{self, NativePipeline, NativeShader};
use super::{backend_error, DirectX12Backend};

pub(crate) fn create_native_pipeline(
    backend: &DirectX12Backend,
    desc: &PipelineDesc,
) -> RhiResult<NativePipeline> {
    let native = backend.native_ref()?;
    let root_signature = native.root_signature.clone();
    if desc.compute_shader.is_some()
        && (desc.vertex_shader.is_some()
            || desc.fragment_shader.is_some()
            || desc.geometry_shader.is_some()
            || desc.tessellation_control_shader.is_some()
            || desc.tessellation_evaluation_shader.is_some())
    {
        return Err(RhiError::InvalidDescription(
            "a Direct3D 12 pipeline cannot mix compute and graphics stages",
        ));
    }
    if let Some(compute_handle) = desc.compute_shader {
        let shader = native
            .shaders
            .get(&compute_handle)
            .ok_or(RhiError::InvalidHandle)?;
        if shader.stage != ShaderStage::Compute {
            return Err(RhiError::InvalidDescription(
                "compute pipeline requires a compute shader",
            ));
        }
        let mut compute_desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root_signature.clone())),
            CS: shader_bytecode(shader),
            ..Default::default()
        };
        let result = unsafe {
            native
                .device
                .CreateComputePipelineState::<ID3D12PipelineState>(&compute_desc)
        };
        unsafe { ManuallyDrop::drop(&mut compute_desc.pRootSignature) };
        let pipeline_state =
            result.map_err(|error| backend_error("create Direct3D 12 compute pipeline", error))?;
        return Ok(NativePipeline {
            pipeline_state,
            root_signature,
            desc: desc.clone(),
        });
    }

    let vertex_shader = get_shader(native, desc.vertex_shader, ShaderStage::Vertex)?;
    let fragment_shader = desc
        .fragment_shader
        .map(|handle| native.shaders.get(&handle).ok_or(RhiError::InvalidHandle))
        .transpose()?;
    let geometry_shader = optional_shader(native, desc.geometry_shader, ShaderStage::Geometry)?;
    let tessellation_control_shader = optional_shader(
        native,
        desc.tessellation_control_shader,
        ShaderStage::TessellationControl,
    )?;
    let tessellation_evaluation_shader = optional_shader(
        native,
        desc.tessellation_evaluation_shader,
        ShaderStage::TessellationEvaluation,
    )?;

    let input_elements = create_input_elements(desc)?;
    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    if desc.color_formats.len() > rtv_formats.len() {
        return Err(RhiError::InvalidDescription(
            "Direct3D 12 supports at most eight color attachments",
        ));
    }
    for (slot, format) in desc.color_formats.iter().copied().enumerate() {
        rtv_formats[slot] = context::texture_format(format)?;
    }

    let mut blend_state = D3D12_BLEND_DESC::default();
    for target in &mut blend_state.RenderTarget {
        *target = default_blend_target();
    }
    if let Some(blend) = desc.blend {
        blend_state.RenderTarget[0] = blend_target(blend);
    }

    let mut graphics_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(Some(root_signature.clone())),
        VS: shader_bytecode(vertex_shader),
        PS: fragment_shader.map(shader_bytecode).unwrap_or_default(),
        GS: geometry_shader.map(shader_bytecode).unwrap_or_default(),
        HS: tessellation_control_shader
            .map(shader_bytecode)
            .unwrap_or_default(),
        DS: tessellation_evaluation_shader
            .map(shader_bytecode)
            .unwrap_or_default(),
        BlendState: blend_state,
        SampleMask: u32::MAX,
        RasterizerState: D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: match desc.cull_mode {
                CullMode::None => D3D12_CULL_MODE_NONE,
                CullMode::Front => D3D12_CULL_MODE_FRONT,
                CullMode::Back => D3D12_CULL_MODE_BACK,
            },
            FrontCounterClockwise: if matches!(desc.front_face, FrontFace::CounterClockwise) {
                BOOL(1)
            } else {
                BOOL(0)
            },
            DepthClipEnable: BOOL(1),
            ..Default::default()
        },
        DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: BOOL(desc.depth_test as i32),
            DepthWriteMask: if desc.depth_write {
                D3D12_DEPTH_WRITE_MASK_ALL
            } else {
                D3D12_DEPTH_WRITE_MASK_ZERO
            },
            DepthFunc: compare_function(desc.depth_compare),
            ..Default::default()
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        },
        PrimitiveTopologyType: primitive_topology_type(desc.primitive_topology),
        NumRenderTargets: desc.color_formats.len() as u32,
        RTVFormats: rtv_formats,
        DSVFormat: desc
            .depth_format
            .map(context::texture_format)
            .transpose()?
            .unwrap_or(DXGI_FORMAT_UNKNOWN),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };
    let result = unsafe {
        native
            .device
            .CreateGraphicsPipelineState::<ID3D12PipelineState>(&graphics_desc)
    };
    unsafe { ManuallyDrop::drop(&mut graphics_desc.pRootSignature) };
    let pipeline_state = result.map_err(|error| {
        let debug_messages = context::debug_layer_messages(&native.device);
        if debug_messages.is_empty() {
            backend_error("create Direct3D 12 graphics pipeline", error)
        } else {
            backend_error(
                "create Direct3D 12 graphics pipeline",
                format!("{error}; debug layer: {}", debug_messages.join(" | ")),
            )
        }
    })?;
    Ok(NativePipeline {
        pipeline_state,
        root_signature,
        desc: desc.clone(),
    })
}

fn get_shader(
    native: &context::NativeState,
    handle: Option<ShaderHandle>,
    expected_stage: ShaderStage,
) -> RhiResult<&NativeShader> {
    let handle = handle.ok_or(RhiError::InvalidDescription(
        "graphics pipeline requires a vertex shader",
    ))?;
    let shader = native.shaders.get(&handle).ok_or(RhiError::InvalidHandle)?;
    if shader.stage != expected_stage {
        return Err(RhiError::InvalidDescription(
            "shader stage does not match the pipeline slot",
        ));
    }
    Ok(shader)
}

fn optional_shader(
    native: &context::NativeState,
    handle: Option<ShaderHandle>,
    stage: ShaderStage,
) -> RhiResult<Option<&NativeShader>> {
    let Some(handle) = handle else {
        return Ok(None);
    };
    let shader = native.shaders.get(&handle).ok_or(RhiError::InvalidHandle)?;
    if shader.stage != stage {
        return Err(RhiError::InvalidDescription(
            "shader stage does not match the pipeline slot",
        ));
    }
    Ok(Some(shader))
}

fn shader_bytecode(shader: &NativeShader) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: shader.bytecode.as_ptr().cast(),
        BytecodeLength: shader.bytecode.len(),
    }
}

fn create_input_elements(desc: &PipelineDesc) -> RhiResult<Vec<D3D12_INPUT_ELEMENT_DESC>> {
    let mut elements = Vec::new();
    for layout in &desc.vertex_buffers {
        for attribute in &layout.attributes {
            elements.push(D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("TEXCOORD"),
                SemanticIndex: attribute.location,
                Format: vertex_format(attribute.format)?,
                InputSlot: attribute.binding,
                AlignedByteOffset: attribute.offset,
                InputSlotClass: match layout.step_mode {
                    VertexStepMode::Vertex => D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                    VertexStepMode::Instance => D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
                },
                InstanceDataStepRate: match layout.step_mode {
                    VertexStepMode::Vertex => 0,
                    VertexStepMode::Instance => 1,
                },
            });
        }
    }
    Ok(elements)
}

pub(crate) fn vertex_format(format: VertexFormat) -> RhiResult<DXGI_FORMAT> {
    Ok(match format {
        VertexFormat::Float32 => DXGI_FORMAT_R32_FLOAT,
        VertexFormat::Float32x2 => DXGI_FORMAT_R32G32_FLOAT,
        VertexFormat::Float32x3 => DXGI_FORMAT_R32G32B32_FLOAT,
        VertexFormat::Float32x4 => DXGI_FORMAT_R32G32B32A32_FLOAT,
        VertexFormat::Uint32 => DXGI_FORMAT_R32_UINT,
        VertexFormat::Uint32x2 => DXGI_FORMAT_R32G32_UINT,
        VertexFormat::Uint32x3 => DXGI_FORMAT_R32G32B32_UINT,
        VertexFormat::Uint32x4 => DXGI_FORMAT_R32G32B32A32_UINT,
        VertexFormat::Uint8x4Norm => DXGI_FORMAT_R8G8B8A8_UNORM,
    })
}

pub(crate) fn primitive_topology_type(
    topology: PrimitiveTopology,
) -> D3D12_PRIMITIVE_TOPOLOGY_TYPE {
    match topology {
        PrimitiveTopology::PointList => D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT,
        PrimitiveTopology::LineList | PrimitiveTopology::LineStrip => {
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_LINE
        }
        PrimitiveTopology::TriangleList | PrimitiveTopology::TriangleStrip => {
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE
        }
        PrimitiveTopology::PatchList => D3D12_PRIMITIVE_TOPOLOGY_TYPE_PATCH,
    }
}

pub(crate) fn primitive_topology(topology: PrimitiveTopology) -> D3D_PRIMITIVE_TOPOLOGY {
    match topology {
        PrimitiveTopology::PointList => D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
        PrimitiveTopology::LineList => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
        PrimitiveTopology::LineStrip => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
        PrimitiveTopology::TriangleList => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        PrimitiveTopology::TriangleStrip => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
        PrimitiveTopology::PatchList => D3D_PRIMITIVE_TOPOLOGY_1_CONTROL_POINT_PATCHLIST,
    }
}

fn compare_function(compare: CompareFunction) -> D3D12_COMPARISON_FUNC {
    match compare {
        CompareFunction::Never => D3D12_COMPARISON_FUNC_NEVER,
        CompareFunction::Less => D3D12_COMPARISON_FUNC_LESS,
        CompareFunction::Equal => D3D12_COMPARISON_FUNC_EQUAL,
        CompareFunction::LessEqual => D3D12_COMPARISON_FUNC_LESS_EQUAL,
        CompareFunction::Greater => D3D12_COMPARISON_FUNC_GREATER,
        CompareFunction::NotEqual => D3D12_COMPARISON_FUNC_NOT_EQUAL,
        CompareFunction::GreaterEqual => D3D12_COMPARISON_FUNC_GREATER_EQUAL,
        CompareFunction::Always => D3D12_COMPARISON_FUNC_ALWAYS,
    }
}

fn default_blend_target() -> D3D12_RENDER_TARGET_BLEND_DESC {
    D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: BOOL(0),
        LogicOpEnable: BOOL(0),
        SrcBlend: D3D12_BLEND_ONE,
        DestBlend: D3D12_BLEND_ZERO,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_ZERO,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    }
}

fn blend_target(blend: BlendState) -> D3D12_RENDER_TARGET_BLEND_DESC {
    D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: BOOL(1),
        LogicOpEnable: BOOL(0),
        SrcBlend: blend_factor(blend.color.src_factor),
        DestBlend: blend_factor(blend.color.dst_factor),
        BlendOp: blend_operation(blend.color.operation),
        SrcBlendAlpha: blend_factor(blend.alpha.src_factor),
        DestBlendAlpha: blend_factor(blend.alpha.dst_factor),
        BlendOpAlpha: blend_operation(blend.alpha.operation),
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    }
}

fn blend_factor(factor: BlendFactor) -> D3D12_BLEND {
    match factor {
        BlendFactor::Zero => D3D12_BLEND_ZERO,
        BlendFactor::One => D3D12_BLEND_ONE,
        BlendFactor::SrcColor => D3D12_BLEND_SRC_COLOR,
        BlendFactor::OneMinusSrcColor => D3D12_BLEND_INV_SRC_COLOR,
        BlendFactor::DstColor => D3D12_BLEND_DEST_COLOR,
        BlendFactor::OneMinusDstColor => D3D12_BLEND_INV_DEST_COLOR,
        BlendFactor::SrcAlpha => D3D12_BLEND_SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => D3D12_BLEND_INV_SRC_ALPHA,
        BlendFactor::DstAlpha => D3D12_BLEND_DEST_ALPHA,
        BlendFactor::OneMinusDstAlpha => D3D12_BLEND_INV_DEST_ALPHA,
        BlendFactor::SrcAlphaSaturated => D3D12_BLEND_SRC_ALPHA_SAT,
        BlendFactor::ConstantColor | BlendFactor::ConstantAlpha => D3D12_BLEND_BLEND_FACTOR,
        BlendFactor::OneMinusConstantColor | BlendFactor::OneMinusConstantAlpha => {
            D3D12_BLEND_INV_BLEND_FACTOR
        }
    }
}

fn blend_operation(operation: BlendOperation) -> D3D12_BLEND_OP {
    match operation {
        BlendOperation::Add => D3D12_BLEND_OP_ADD,
        BlendOperation::Subtract => D3D12_BLEND_OP_SUBTRACT,
        BlendOperation::ReverseSubtract => D3D12_BLEND_OP_REV_SUBTRACT,
        BlendOperation::Min => D3D12_BLEND_OP_MIN,
        BlendOperation::Max => D3D12_BLEND_OP_MAX,
    }
}
