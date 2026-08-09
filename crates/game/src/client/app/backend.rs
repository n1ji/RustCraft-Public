use renderer_api::rhi::command::{CommandBuffer, CommandEncoder};
use renderer_api::rhi::device::{CommandQueue, RenderDevice};
use renderer_api::rhi::types::*;

pub enum RenderBackend {
    Vulkan(renderer_vulkan::VulkanBackend),
    #[cfg(windows)]
    DirectX12(renderer_dx12::DirectX12Backend),
}

impl RenderBackend {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Vulkan(_) => "Vulkan",
            #[cfg(windows)]
            Self::DirectX12(_) => "DirectX 12",
        }
    }
}

impl CommandQueue for RenderBackend {
    fn submit(&mut self, command_buffer: CommandBuffer) -> RhiResult<SubmissionId> {
        match self {
            Self::Vulkan(backend) => backend.submit(command_buffer),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.submit(command_buffer),
        }
    }

    fn collect_completed(&mut self) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.collect_completed(),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.collect_completed(),
        }
    }

    fn wait_idle(&mut self) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.wait_idle(),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.wait_idle(),
        }
    }
}

impl RenderDevice for RenderBackend {
    fn capabilities(&self) -> DeviceCapabilities {
        match self {
            Self::Vulkan(backend) => backend.capabilities(),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.capabilities(),
        }
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle> {
        match self {
            Self::Vulkan(backend) => backend.create_buffer(desc),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.create_buffer(desc),
        }
    }

    fn create_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        match self {
            Self::Vulkan(backend) => backend.create_texture(desc),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.create_texture(desc),
        }
    }

    fn create_shader(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle> {
        match self {
            Self::Vulkan(backend) => backend.create_shader(desc),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.create_shader(desc),
        }
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> RhiResult<PipelineHandle> {
        match self {
            Self::Vulkan(backend) => backend.create_pipeline(desc),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.create_pipeline(desc),
        }
    }

    fn create_sampler(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle> {
        match self {
            Self::Vulkan(backend) => backend.create_sampler(desc),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.create_sampler(desc),
        }
    }

    fn create_bind_group(&mut self, desc: BindGroupDesc) -> RhiResult<BindGroupHandle> {
        match self {
            Self::Vulkan(backend) => backend.create_bind_group(desc),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.create_bind_group(desc),
        }
    }

    fn configure_surface(&mut self, config: SurfaceConfig) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.configure_surface(config),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.configure_surface(config),
        }
    }

    fn acquire_surface_frame(&mut self) -> RhiResult<SurfaceFrame> {
        match self {
            Self::Vulkan(backend) => backend.acquire_surface_frame(),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.acquire_surface_frame(),
        }
    }

    fn present(&mut self, frame: SurfaceFrame) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.present(frame),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.present(frame),
        }
    }

    fn upload_texture(
        &mut self,
        handle: TextureHandle,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.upload_texture(handle, data, row_pitch),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.upload_texture(handle, data, row_pitch),
        }
    }

    fn upload_texture_region(
        &mut self,
        handle: TextureHandle,
        origin: [u32; 2],
        extent: Extent2D,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => {
                backend.upload_texture_region(handle, origin, extent, data, row_pitch)
            }
            #[cfg(windows)]
            Self::DirectX12(backend) => {
                backend.upload_texture_region(handle, origin, extent, data, row_pitch)
            }
        }
    }

    fn upload_buffer(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.upload_buffer(handle, offset, data),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.upload_buffer(handle, offset, data),
        }
    }

    fn destroy_buffer(&mut self, handle: BufferHandle) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.destroy_buffer(handle),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.destroy_buffer(handle),
        }
    }

    fn destroy_texture(&mut self, handle: TextureHandle) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.destroy_texture(handle),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.destroy_texture(handle),
        }
    }

    fn destroy_shader(&mut self, handle: ShaderHandle) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.destroy_shader(handle),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.destroy_shader(handle),
        }
    }

    fn destroy_pipeline(&mut self, handle: PipelineHandle) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.destroy_pipeline(handle),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.destroy_pipeline(handle),
        }
    }

    fn destroy_sampler(&mut self, handle: SamplerHandle) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.destroy_sampler(handle),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.destroy_sampler(handle),
        }
    }

    fn destroy_bind_group(&mut self, handle: BindGroupHandle) -> RhiResult<()> {
        match self {
            Self::Vulkan(backend) => backend.destroy_bind_group(handle),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.destroy_bind_group(handle),
        }
    }

    fn begin_commands(&mut self, label: &str) -> RhiResult<Box<dyn CommandEncoder>> {
        match self {
            Self::Vulkan(backend) => backend.begin_commands(label),
            #[cfg(windows)]
            Self::DirectX12(backend) => backend.begin_commands(label),
        }
    }
}
