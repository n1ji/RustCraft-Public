use std::collections::HashMap;

use ash::vk;

use renderer::rhi::types::*;

use super::context::{NativePipeline, NativeShader};

pub(crate) fn merge_shader_interfaces(
    shaders: &HashMap<ShaderHandle, NativeShader>,
    desc: &PipelineDesc,
) -> RhiResult<ShaderInterface> {
    let handles = [
        desc.vertex_shader,
        desc.fragment_shader,
        desc.geometry_shader,
        desc.tessellation_control_shader,
        desc.tessellation_evaluation_shader,
        desc.compute_shader,
    ];
    let mut merged = ShaderInterface::default();
    for handle in handles.into_iter().flatten() {
        let Some(shader) = shaders.get(&handle) else {
            return Err(RhiError::InvalidHandle);
        };
        merged.push_constant_size = merged
            .push_constant_size
            .max(shader.interface.push_constant_size);
        for binding in &shader.interface.bindings {
            if let Some(existing) = merged
                .bindings
                .iter_mut()
                .find(|existing| existing.set == binding.set && existing.binding == binding.binding)
            {
                if existing.ty != binding.ty || existing.array_size != binding.array_size {
                    return Err(RhiError::InvalidDescription(
                        "shader stages disagree about a resource binding",
                    ));
                }
                existing.stages |= binding.stages;
            } else {
                merged.bindings.push(binding.clone());
            }
        }
    }
    Ok(merged)
}

pub(crate) fn descriptor_type(binding_type: ShaderBindingType) -> vk::DescriptorType {
    match binding_type {
        ShaderBindingType::UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
        ShaderBindingType::StorageBuffer => vk::DescriptorType::STORAGE_BUFFER,
        ShaderBindingType::SampledTexture => vk::DescriptorType::SAMPLED_IMAGE,
        ShaderBindingType::StorageTexture => vk::DescriptorType::STORAGE_IMAGE,
        ShaderBindingType::Sampler => vk::DescriptorType::SAMPLER,
        ShaderBindingType::CombinedImageSampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
        ShaderBindingType::AccelerationStructure => vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
    }
}

fn shader_stage_flags(stages: ShaderStages) -> vk::ShaderStageFlags {
    let mut flags = vk::ShaderStageFlags::empty();
    if stages.contains(ShaderStages::VERTEX) {
        flags |= vk::ShaderStageFlags::VERTEX;
    }
    if stages.contains(ShaderStages::FRAGMENT) {
        flags |= vk::ShaderStageFlags::FRAGMENT;
    }
    if stages.contains(ShaderStages::GEOMETRY) {
        flags |= vk::ShaderStageFlags::GEOMETRY;
    }
    if stages.contains(ShaderStages::TESSELLATION_CONTROL) {
        flags |= vk::ShaderStageFlags::TESSELLATION_CONTROL;
    }
    if stages.contains(ShaderStages::TESSELLATION_EVALUATION) {
        flags |= vk::ShaderStageFlags::TESSELLATION_EVALUATION;
    }
    if stages.contains(ShaderStages::COMPUTE) {
        flags |= vk::ShaderStageFlags::COMPUTE;
    }
    flags
}

fn destroy_pipeline_layout(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    set_layouts: Vec<vk::DescriptorSetLayout>,
) {
    unsafe {
        device.destroy_pipeline_layout(layout, None);
        for set_layout in set_layouts {
            device.destroy_descriptor_set_layout(set_layout, None);
        }
    }
}

fn create_pipeline_layout(
    device: &ash::Device,
    interface: &ShaderInterface,
) -> RhiResult<(vk::PipelineLayout, Vec<vk::DescriptorSetLayout>)> {
    let set_count = interface.max_set().map_or(0, |set| set + 1);
    let mut set_layouts = Vec::with_capacity(set_count as usize);
    for set in 0..set_count {
        let bindings = interface
            .bindings
            .iter()
            .filter(|binding| binding.set == set)
            .map(|binding| {
                let stage_flags = shader_stage_flags(binding.stages);
                if stage_flags.is_empty() {
                    return Err(RhiError::InvalidDescription(
                        "shader binding must declare at least one stage",
                    ));
                }
                Ok(vk::DescriptorSetLayoutBinding::default()
                    .binding(binding.binding)
                    .descriptor_type(descriptor_type(binding.ty))
                    .descriptor_count(binding.array_size.max(1))
                    .stage_flags(stage_flags))
            })
            .collect::<RhiResult<Vec<_>>>()?;
        let layout = match unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
        } {
            Ok(layout) => layout,
            Err(error) => {
                for layout in set_layouts.drain(..) {
                    unsafe { device.destroy_descriptor_set_layout(layout, None) };
                }
                return Err(RhiError::Backend(format!(
                    "create Vulkan descriptor set layout: {error:?}"
                )));
            }
        };
        set_layouts.push(layout);
    }

    let push_constant_ranges = if interface.push_constant_size == 0 {
        Vec::new()
    } else {
        vec![vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::ALL_GRAPHICS | vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(interface.push_constant_size)]
    };
    let create_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&set_layouts)
        .push_constant_ranges(&push_constant_ranges);
    let layout = match unsafe { device.create_pipeline_layout(&create_info, None) } {
        Ok(layout) => layout,
        Err(error) => {
            for set_layout in set_layouts.drain(..) {
                unsafe { device.destroy_descriptor_set_layout(set_layout, None) };
            }
            return Err(RhiError::Backend(format!(
                "create Vulkan pipeline layout: {error:?}"
            )));
        }
    };
    Ok((layout, set_layouts))
}

pub(crate) fn create_native_pipeline(
    device: &ash::Device,
    shaders: &HashMap<ShaderHandle, NativeShader>,
    desc: &PipelineDesc,
) -> RhiResult<NativePipeline> {
    let interface = merge_shader_interfaces(shaders, desc)?;
    let (layout, set_layouts) = create_pipeline_layout(device, &interface)?;

    if let Some(compute_handle) = desc.compute_shader {
        let Some(shader) = shaders.get(&compute_handle) else {
            destroy_pipeline_layout(device, layout, set_layouts);
            return Err(RhiError::InvalidHandle);
        };
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader.module)
            .name(shader.entry_point.as_c_str());
        let create_info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(layout);
        let result = unsafe {
            device.create_compute_pipelines(vk::PipelineCache::null(), &[create_info], None)
        };
        return match result {
            Ok(mut pipelines) => pipelines
                .pop()
                .map(|pipeline| NativePipeline {
                    pipeline,
                    layout,
                    set_layouts,
                    bind_point: vk::PipelineBindPoint::COMPUTE,
                })
                .ok_or_else(|| RhiError::Backend(String::from("Vulkan returned no pipeline"))),
            Err((_, error)) => {
                destroy_pipeline_layout(device, layout, set_layouts);
                Err(RhiError::Backend(format!(
                    "create Vulkan compute pipeline: {error:?}"
                )))
            }
        };
    }

    let Some(vertex_handle) = desc.vertex_shader else {
        destroy_pipeline_layout(device, layout, set_layouts);
        return Err(RhiError::InvalidDescription(
            "graphics pipeline requires a vertex shader",
        ));
    };
    let Some(vertex_shader) = shaders.get(&vertex_handle) else {
        destroy_pipeline_layout(device, layout, set_layouts);
        return Err(RhiError::InvalidHandle);
    };
    let mut stages = vec![vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::VERTEX)
        .module(vertex_shader.module)
        .name(vertex_shader.entry_point.as_c_str())];
    let optional_stages = [
        (
            desc.fragment_shader,
            ShaderStage::Fragment,
            vk::ShaderStageFlags::FRAGMENT,
        ),
        (
            desc.geometry_shader,
            ShaderStage::Geometry,
            vk::ShaderStageFlags::GEOMETRY,
        ),
        (
            desc.tessellation_control_shader,
            ShaderStage::TessellationControl,
            vk::ShaderStageFlags::TESSELLATION_CONTROL,
        ),
        (
            desc.tessellation_evaluation_shader,
            ShaderStage::TessellationEvaluation,
            vk::ShaderStageFlags::TESSELLATION_EVALUATION,
        ),
    ];
    for (handle, expected_stage, stage_flags) in optional_stages {
        let Some(handle) = handle else { continue };
        let Some(shader) = shaders.get(&handle) else {
            destroy_pipeline_layout(device, layout, set_layouts);
            return Err(RhiError::InvalidHandle);
        };
        if shader.stage != expected_stage {
            destroy_pipeline_layout(device, layout, set_layouts);
            return Err(RhiError::InvalidDescription(
                "graphics pipeline shader stage does not match its slot",
            ));
        }
        stages.push(
            vk::PipelineShaderStageCreateInfo::default()
                .stage(stage_flags)
                .module(shader.module)
                .name(shader.entry_point.as_c_str()),
        );
    }

    let color_formats = desc
        .color_formats
        .iter()
        .copied()
        .map(super::resources::texture_format)
        .collect::<Vec<_>>();
    let mut rendering_info =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&color_formats);
    if let Some(depth_format) = desc.depth_format {
        rendering_info =
            rendering_info.depth_attachment_format(super::resources::texture_format(depth_format));
    }
    let vertex_bindings = desc
        .vertex_buffers
        .iter()
        .map(|layout| vk::VertexInputBindingDescription {
            binding: layout.binding,
            stride: layout.stride,
            input_rate: match layout.step_mode {
                VertexStepMode::Vertex => vk::VertexInputRate::VERTEX,
                VertexStepMode::Instance => vk::VertexInputRate::INSTANCE,
            },
        })
        .collect::<Vec<_>>();
    let vertex_attributes = desc
        .vertex_buffers
        .iter()
        .flat_map(|layout| {
            layout
                .attributes
                .iter()
                .map(|attribute| vk::VertexInputAttributeDescription {
                    location: attribute.location,
                    binding: attribute.binding,
                    format: vertex_format(attribute.format),
                    offset: attribute.offset,
                })
        })
        .collect::<Vec<_>>();
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&vertex_bindings)
        .vertex_attribute_descriptions(&vertex_attributes);
    let has_tessellation =
        desc.tessellation_control_shader.is_some() || desc.tessellation_evaluation_shader.is_some();
    let input_assembly =
        vk::PipelineInputAssemblyStateCreateInfo::default().topology(if has_tessellation {
            vk::PrimitiveTopology::PATCH_LIST
        } else {
            primitive_topology(desc.primitive_topology)
        });
    let tessellation = vk::PipelineTessellationStateCreateInfo::default().patch_control_points(3);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(cull_mode(desc.cull_mode))
        .front_face(front_face(desc.front_face))
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(desc.depth_format.is_some() && desc.depth_test)
        .depth_write_enable(desc.depth_format.is_some() && desc.depth_write)
        .depth_compare_op(compare_function(desc.depth_compare));
    let color_blend_attachments = color_formats
        .iter()
        .map(|_| {
            let mut state = vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA);
            if let Some(blend) = desc.blend {
                state = state
                    .blend_enable(true)
                    .src_color_blend_factor(blend_factor(blend.color.src_factor))
                    .dst_color_blend_factor(blend_factor(blend.color.dst_factor))
                    .color_blend_op(blend_operation(blend.color.operation))
                    .src_alpha_blend_factor(blend_factor(blend.alpha.src_factor))
                    .dst_alpha_blend_factor(blend_factor(blend.alpha.dst_factor))
                    .alpha_blend_op(blend_operation(blend.alpha.operation));
            }
            state
        })
        .collect::<Vec<_>>();
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_blend_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let mut create_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth_stencil)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(layout)
        .push_next(&mut rendering_info);
    if has_tessellation {
        create_info = create_info.tessellation_state(&tessellation);
    }
    match unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[create_info], None)
    } {
        Ok(mut pipelines) => pipelines
            .pop()
            .map(|pipeline| NativePipeline {
                pipeline,
                layout,
                set_layouts,
                bind_point: vk::PipelineBindPoint::GRAPHICS,
            })
            .ok_or_else(|| RhiError::Backend(String::from("Vulkan returned no graphics pipeline"))),
        Err((_, error)) => {
            destroy_pipeline_layout(device, layout, set_layouts);
            Err(RhiError::Backend(format!(
                "create Vulkan graphics pipeline: {error:?}"
            )))
        }
    }
}

fn vertex_format(format: VertexFormat) -> vk::Format {
    match format {
        VertexFormat::Float32 => vk::Format::R32_SFLOAT,
        VertexFormat::Float32x2 => vk::Format::R32G32_SFLOAT,
        VertexFormat::Float32x3 => vk::Format::R32G32B32_SFLOAT,
        VertexFormat::Float32x4 => vk::Format::R32G32B32A32_SFLOAT,
        VertexFormat::Uint32 => vk::Format::R32_UINT,
        VertexFormat::Uint32x2 => vk::Format::R32G32_UINT,
        VertexFormat::Uint32x3 => vk::Format::R32G32B32_UINT,
        VertexFormat::Uint32x4 => vk::Format::R32G32B32A32_UINT,
        VertexFormat::Uint8x4Norm => vk::Format::R8G8B8A8_UNORM,
    }
}

fn primitive_topology(topology: PrimitiveTopology) -> vk::PrimitiveTopology {
    match topology {
        PrimitiveTopology::PointList => vk::PrimitiveTopology::POINT_LIST,
        PrimitiveTopology::LineList => vk::PrimitiveTopology::LINE_LIST,
        PrimitiveTopology::LineStrip => vk::PrimitiveTopology::LINE_STRIP,
        PrimitiveTopology::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
        PrimitiveTopology::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
        PrimitiveTopology::PatchList => vk::PrimitiveTopology::PATCH_LIST,
    }
}

fn cull_mode(mode: CullMode) -> vk::CullModeFlags {
    match mode {
        CullMode::None => vk::CullModeFlags::NONE,
        CullMode::Front => vk::CullModeFlags::FRONT,
        CullMode::Back => vk::CullModeFlags::BACK,
    }
}

fn front_face(face: FrontFace) -> vk::FrontFace {
    match face {
        FrontFace::CounterClockwise => vk::FrontFace::COUNTER_CLOCKWISE,
        FrontFace::Clockwise => vk::FrontFace::CLOCKWISE,
    }
}

fn compare_function(compare: CompareFunction) -> vk::CompareOp {
    match compare {
        CompareFunction::Never => vk::CompareOp::NEVER,
        CompareFunction::Less => vk::CompareOp::LESS,
        CompareFunction::Equal => vk::CompareOp::EQUAL,
        CompareFunction::LessEqual => vk::CompareOp::LESS_OR_EQUAL,
        CompareFunction::Greater => vk::CompareOp::GREATER,
        CompareFunction::NotEqual => vk::CompareOp::NOT_EQUAL,
        CompareFunction::GreaterEqual => vk::CompareOp::GREATER_OR_EQUAL,
        CompareFunction::Always => vk::CompareOp::ALWAYS,
    }
}

fn blend_factor(factor: BlendFactor) -> vk::BlendFactor {
    match factor {
        BlendFactor::Zero => vk::BlendFactor::ZERO,
        BlendFactor::One => vk::BlendFactor::ONE,
        BlendFactor::SrcColor => vk::BlendFactor::SRC_COLOR,
        BlendFactor::OneMinusSrcColor => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
        BlendFactor::DstColor => vk::BlendFactor::DST_COLOR,
        BlendFactor::OneMinusDstColor => vk::BlendFactor::ONE_MINUS_DST_COLOR,
        BlendFactor::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        BlendFactor::DstAlpha => vk::BlendFactor::DST_ALPHA,
        BlendFactor::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
        BlendFactor::SrcAlphaSaturated => vk::BlendFactor::SRC_ALPHA_SATURATE,
        BlendFactor::ConstantColor => vk::BlendFactor::CONSTANT_COLOR,
        BlendFactor::OneMinusConstantColor => vk::BlendFactor::ONE_MINUS_CONSTANT_COLOR,
        BlendFactor::ConstantAlpha => vk::BlendFactor::CONSTANT_ALPHA,
        BlendFactor::OneMinusConstantAlpha => vk::BlendFactor::ONE_MINUS_CONSTANT_ALPHA,
    }
}

fn blend_operation(operation: BlendOperation) -> vk::BlendOp {
    match operation {
        BlendOperation::Add => vk::BlendOp::ADD,
        BlendOperation::Subtract => vk::BlendOp::SUBTRACT,
        BlendOperation::ReverseSubtract => vk::BlendOp::REVERSE_SUBTRACT,
        BlendOperation::Min => vk::BlendOp::MIN,
        BlendOperation::Max => vk::BlendOp::MAX,
    }
}
