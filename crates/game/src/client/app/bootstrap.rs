use winit::window::Window;

use renderer_api::rhi::device::RenderDevice;

use super::WINDOW_TITLE;

pub(super) fn initialize_renderer(
    window: &Window,
) -> crate::render::GameRenderer<super::RenderBackend> {
    let requested_backend = std::env::var("RUSTCRAFT_RENDER_BACKEND")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let backend = if requested_backend == "dx12" {
        #[cfg(windows)]
        {
            match renderer_dx12::DirectX12Backend::initialize_with_window(
                window,
                renderer_dx12::DirectX12BackendConfig {
                    app_name: String::from(WINDOW_TITLE),
                    enable_debug_layer: cfg!(debug_assertions),
                    allow_warp: false,
                    frame_count: 2,
                },
            ) {
                Ok(backend) => super::RenderBackend::DirectX12(backend),
                Err(error) => {
                    log::warn!(
                        "failed to initialise Direct3D 12 backend: {error}; falling back to Vulkan"
                    );
                    create_vulkan_backend(window)
                }
            }
        }
        #[cfg(not(windows))]
        {
            log::warn!("RUSTCRAFT_RENDER_BACKEND=dx12 is only supported on Windows; using Vulkan");
            create_vulkan_backend(window)
        }
    } else {
        create_vulkan_backend(window)
    };
    log::info!("selected render backend: {}", backend.name());
    let mut renderer = crate::render::GameRenderer::new(backend);
    let gui_vertex = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::GuiVertex.descriptor())
        .expect("failed to create GUI vertex shader");
    let gui_fragment = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::GuiFragment.descriptor())
        .expect("failed to create GUI fragment shader");
    let gui_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(gui_vertex),
            fragment_shader: Some(gui_fragment),
            vertex_buffers: vec![crate::render::gui::GuiVertex::vertex_layout()],
            color_formats: vec![renderer_api::rhi::types::TextureFormat::B8G8R8A8Srgb],
            depth_format: None,
            cull_mode: renderer_api::rhi::types::CullMode::None,
            depth_test: false,
            depth_write: false,
            blend: Some(renderer_api::rhi::types::BlendState::alpha_blend()),
            label: String::from("game.gui.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create GUI pipeline");
    renderer
        .configure_gui_pipeline(gui_pipeline)
        .expect("failed to configure GUI RHI resources");
    let world_vertex = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::BasicVertex.descriptor())
        .expect("failed to create world vertex shader");
    let world_fragment = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::BasicFragment.descriptor())
        .expect("failed to create world fragment shader");
    let world_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(world_vertex),
            fragment_shader: Some(world_fragment),
            vertex_buffers: vec![crate::world::mesh::Vertex::vertex_layout()],
            color_formats: vec![
                renderer_api::rhi::types::TextureFormat::R16G16B16A16Float,
                renderer_api::rhi::types::TextureFormat::R16G16Float,
            ],
            depth_format: Some(renderer_api::rhi::types::TextureFormat::D32Float),
            cull_mode: renderer_api::rhi::types::CullMode::Back,
            depth_test: true,
            depth_write: true,
            depth_compare: renderer_api::rhi::types::CompareFunction::LessEqual,
            label: String::from("game.world.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create world pipeline");
    let world_transparent_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(world_vertex),
            fragment_shader: Some(world_fragment),
            vertex_buffers: vec![crate::world::mesh::Vertex::vertex_layout()],
            color_formats: vec![
                renderer_api::rhi::types::TextureFormat::R16G16B16A16Float,
                renderer_api::rhi::types::TextureFormat::R16G16Float,
            ],
            depth_format: Some(renderer_api::rhi::types::TextureFormat::D32Float),
            cull_mode: renderer_api::rhi::types::CullMode::None,
            depth_test: true,
            depth_write: false,
            depth_compare: renderer_api::rhi::types::CompareFunction::LessEqual,
            blend: Some(renderer_api::rhi::types::BlendState::alpha_blend()),
            label: String::from("game.world-transparent.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create transparent world pipeline");
    renderer
        .configure_world_pipeline(world_pipeline, world_transparent_pipeline)
        .expect("failed to configure world RHI resources");
    let sky_vertex = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::SkyVertex.descriptor())
        .expect("failed to create sky vertex shader");
    let sky_fragment = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::SkyFragment.descriptor())
        .expect("failed to create sky fragment shader");
    let sky_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(sky_vertex),
            fragment_shader: Some(sky_fragment),
            vertex_buffers: vec![renderer_api::rhi::types::VertexBufferLayout::new(
                0,
                8,
                vec![renderer_api::rhi::types::VertexAttribute::new(
                    0,
                    0,
                    renderer_api::rhi::types::VertexFormat::Float32x2,
                    0,
                )],
            )],
            color_formats: vec![
                renderer_api::rhi::types::TextureFormat::R16G16B16A16Float,
                renderer_api::rhi::types::TextureFormat::R16G16Float,
            ],
            // Sky is a depth-independent background pass.  It deliberately
            // has no depth attachment; world geometry initializes depth in
            // the following pass.
            depth_format: None,
            cull_mode: renderer_api::rhi::types::CullMode::None,
            depth_test: false,
            depth_write: false,
            label: String::from("game.sky.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create sky pipeline");
    renderer
        .configure_sky_pipeline(sky_pipeline)
        .expect("failed to configure sky RHI resources");
    let entity_vertex = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::EntityVertex.descriptor())
        .expect("failed to create entity vertex shader");
    let entity_fragment = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::EntityFragment.descriptor())
        .expect("failed to create entity fragment shader");
    let entity_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(entity_vertex),
            fragment_shader: Some(entity_fragment),
            vertex_buffers: vec![crate::render::entity::mesh::EntityVertex::vertex_layout()],
            color_formats: vec![
                renderer_api::rhi::types::TextureFormat::R16G16B16A16Float,
                renderer_api::rhi::types::TextureFormat::R16G16Float,
            ],
            depth_format: Some(renderer_api::rhi::types::TextureFormat::D32Float),
            cull_mode: renderer_api::rhi::types::CullMode::Back,
            depth_test: true,
            depth_write: true,
            depth_compare: renderer_api::rhi::types::CompareFunction::LessEqual,
            label: String::from("game.entity.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create entity pipeline");
    let entity_transparent_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(entity_vertex),
            fragment_shader: Some(entity_fragment),
            vertex_buffers: vec![crate::render::entity::mesh::EntityVertex::vertex_layout()],
            color_formats: vec![
                renderer_api::rhi::types::TextureFormat::R16G16B16A16Float,
                renderer_api::rhi::types::TextureFormat::R16G16Float,
            ],
            depth_format: Some(renderer_api::rhi::types::TextureFormat::D32Float),
            cull_mode: renderer_api::rhi::types::CullMode::None,
            depth_test: true,
            depth_write: false,
            depth_compare: renderer_api::rhi::types::CompareFunction::LessEqual,
            blend: Some(renderer_api::rhi::types::BlendState::alpha_blend()),
            label: String::from("game.entity-transparent.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create transparent entity pipeline");
    let entity_first_person_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(entity_vertex),
            fragment_shader: Some(entity_fragment),
            vertex_buffers: vec![crate::render::entity::mesh::EntityVertex::vertex_layout()],
            color_formats: vec![
                renderer_api::rhi::types::TextureFormat::R16G16B16A16Float,
                renderer_api::rhi::types::TextureFormat::R16G16Float,
            ],
            depth_format: Some(renderer_api::rhi::types::TextureFormat::D32Float),
            // Vanilla disables face culling for the arm, while the dedicated
            // hand pass clears depth and keeps writing it as geometry is drawn.
            cull_mode: renderer_api::rhi::types::CullMode::None,
            depth_test: true,
            depth_write: true,
            depth_compare: renderer_api::rhi::types::CompareFunction::LessEqual,
            blend: Some(renderer_api::rhi::types::BlendState::alpha_blend()),
            label: String::from("game.entity-first-person.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create first-person entity pipeline");
    renderer
        .configure_entity_pipelines(
            entity_pipeline,
            entity_transparent_pipeline,
            entity_first_person_pipeline,
        )
        .expect("failed to configure entity RHI resources");

    // Keep the HDR world target linear until the final pass.  Without this
    // pass R16G16B16A16_FLOAT is copied directly into the sRGB swapchain;
    // daylight and block-light values above 1.0 then clamp to white, which
    // makes lighting and alpha-blended water/glass look completely wrong.
    let tone_vertex = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::ToneMappingVertex.descriptor())
        .expect("failed to create tone-mapping vertex shader");
    let tone_fragment = renderer
        .runtime_mut()
        .create_shader(renderer_vulkan::shader::BuiltinShader::ToneMappingFragment.descriptor())
        .expect("failed to create tone-mapping fragment shader");
    let tone_pipeline = renderer
        .runtime_mut()
        .create_pipeline(renderer_api::rhi::types::PipelineDesc {
            vertex_shader: Some(tone_vertex),
            fragment_shader: Some(tone_fragment),
            vertex_buffers: vec![renderer_api::rhi::types::VertexBufferLayout::new(
                0,
                8,
                vec![renderer_api::rhi::types::VertexAttribute::new(
                    0,
                    0,
                    renderer_api::rhi::types::VertexFormat::Float32x2,
                    0,
                )],
            )],
            color_formats: vec![renderer_api::rhi::types::TextureFormat::B8G8R8A8Srgb],
            cull_mode: renderer_api::rhi::types::CullMode::None,
            depth_format: None,
            depth_test: false,
            depth_write: false,
            label: String::from("game.tone-mapping.pipeline"),
            ..renderer_api::rhi::types::PipelineDesc::default()
        })
        .expect("failed to create tone-mapping pipeline");
    let tone_sampler = renderer
        .runtime_mut()
        .create_sampler(renderer_api::rhi::types::SamplerDesc {
            // Render and display extents are identical until an explicit
            // upscaler pass is installed. A nearest final sample preserves
            // the block atlas texels instead of filtering the whole frame.
            linear_filter: false,
            repeat_u: false,
            repeat_v: false,
            repeat_w: false,
            label: String::from("game.tone-mapping.sampler"),
        })
        .expect("failed to create tone-mapping sampler");
    let tone_vertices: [[f32; 2]; 3] = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
    let tone_vertex_buffer = renderer
        .runtime_mut()
        .create_buffer(renderer_api::rhi::types::BufferDesc::new(
            std::mem::size_of_val(&tone_vertices) as u64,
            renderer_api::rhi::types::BufferUsage::VERTEX
                | renderer_api::rhi::types::BufferUsage::TRANSFER_DST,
            "game.tone-mapping.vertices",
        ))
        .expect("failed to create tone-mapping vertex buffer");
    renderer
        .runtime_mut()
        .device()
        .upload_buffer(tone_vertex_buffer, 0, bytemuck::cast_slice(&tone_vertices))
        .expect("failed to upload tone-mapping vertex buffer");
    renderer
        .configure_tone_mapping(tone_pipeline, tone_sampler, tone_vertex_buffer)
        .expect("failed to configure tone-mapping pass");
    renderer
}

fn create_vulkan_backend(window: &Window) -> super::RenderBackend {
    // Validation is valuable while diagnosing backend changes, but enabling
    // it on every debug launch adds a large CPU cost to every Vulkan command
    // and makes frame pacing look worse than the actual renderer. Opt in with
    // RUSTCRAFT_VULKAN_VALIDATION=1 when validation is needed.
    let enable_validation = std::env::var("RUSTCRAFT_VULKAN_VALIDATION")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
    log::info!(
        "Vulkan validation layers: {}",
        if enable_validation {
            "enabled"
        } else {
            "disabled"
        }
    );
    let backend = renderer_vulkan::VulkanBackend::initialize_with_window(
        window,
        renderer_vulkan::VulkanBackendConfig {
            app_name: String::from(WINDOW_TITLE),
            enable_validation,
        },
    )
    .expect("failed to initialise renderer backend");
    super::RenderBackend::Vulkan(backend)
}
