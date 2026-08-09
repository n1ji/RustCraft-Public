use super::command::{CommandBuffer, CommandEncoder};
use super::types::{
    BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, DeviceCapabilities, Extent2D,
    PipelineDesc, PipelineHandle, RhiResult, SamplerDesc, SamplerHandle, ShaderDesc, ShaderHandle,
    SurfaceConfig, SurfaceFrame, TextureDesc, TextureHandle,
};

pub trait CommandQueue {
    fn submit(&mut self, command_buffer: CommandBuffer) -> RhiResult<super::types::SubmissionId>;

    /// Reclaim backend command resources whose GPU submission has completed.
    /// Backends without asynchronous command ownership may use the default.
    fn collect_completed(&mut self) -> RhiResult<()> {
        Ok(())
    }

    fn wait_idle(&mut self) -> RhiResult<()>;
}

pub trait RenderDevice: CommandQueue {
    fn capabilities(&self) -> DeviceCapabilities;

    fn create_buffer(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle>;
    fn create_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle>;
    fn create_shader(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle>;
    fn create_pipeline(&mut self, desc: PipelineDesc) -> RhiResult<PipelineHandle>;
    fn create_sampler(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle>;
    fn create_bind_group(&mut self, desc: BindGroupDesc) -> RhiResult<BindGroupHandle>;

    fn configure_surface(&mut self, _config: SurfaceConfig) -> RhiResult<()> {
        Err(super::types::RhiError::Message(String::from(
            "the selected backend does not expose a surface",
        )))
    }

    fn acquire_surface_frame(&mut self) -> RhiResult<SurfaceFrame> {
        Err(super::types::RhiError::Message(String::from(
            "the selected backend does not expose a surface",
        )))
    }

    fn present(&mut self, _frame: SurfaceFrame) -> RhiResult<()> {
        Err(super::types::RhiError::Message(String::from(
            "the selected backend does not expose a surface",
        )))
    }

    /// Upload tightly packed 2D texels into a texture resource.
    ///
    /// `row_pitch` is expressed in bytes and lets a backend preserve source
    /// row alignment without exposing its native staging objects.
    fn upload_texture(
        &mut self,
        handle: TextureHandle,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()>;

    /// Upload a tightly packed rectangular region into an existing 2D texture.
    ///
    /// The default keeps non-rendering test backends source-compatible. Real
    /// backends should override this when they advertise texture uploads.
    fn upload_texture_region(
        &mut self,
        _handle: TextureHandle,
        _origin: [u32; 2],
        _extent: Extent2D,
        _data: &[u8],
        _row_pitch: u32,
    ) -> RhiResult<()> {
        Err(super::types::RhiError::Message(String::from(
            "partial texture uploads are not supported by this backend",
        )))
    }

    /// Upload bytes into a host-visible buffer resource.
    fn upload_buffer(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> RhiResult<()>;

    fn destroy_buffer(&mut self, handle: BufferHandle) -> RhiResult<()>;
    fn destroy_texture(&mut self, handle: TextureHandle) -> RhiResult<()>;
    fn destroy_shader(&mut self, handle: ShaderHandle) -> RhiResult<()>;
    fn destroy_pipeline(&mut self, handle: PipelineHandle) -> RhiResult<()>;
    fn destroy_sampler(&mut self, handle: SamplerHandle) -> RhiResult<()>;
    fn destroy_bind_group(&mut self, handle: BindGroupHandle) -> RhiResult<()>;

    fn begin_commands(&mut self, label: &str) -> RhiResult<Box<dyn CommandEncoder>>;
}
