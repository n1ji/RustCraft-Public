use super::types::{
    BindGroupHandle, BufferHandle, PipelineHandle, ResourceBarrier, RhiError, RhiResult,
    TextureHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadOp {
    Load,
    Clear,
    DontCare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreOp {
    Store,
    DontCare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearValue {
    Color([u32; 4]),
    DepthStencil { depth: u32, stencil: u32 },
}

impl ClearValue {
    pub fn color(value: [f32; 4]) -> Self {
        Self::Color(value.map(f32::to_bits))
    }

    pub fn depth_stencil(depth: f32, stencil: u32) -> Self {
        Self::DepthStencil {
            depth: depth.to_bits(),
            stencil,
        }
    }
}

impl Default for ClearValue {
    fn default() -> Self {
        Self::Color([0; 4])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderAttachment {
    pub texture: TextureHandle,
    pub load: LoadOp,
    pub store: StoreOp,
    pub clear: ClearValue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderPassDesc {
    pub label: String,
    pub colors: Vec<RenderAttachment>,
    pub depth: Option<RenderAttachment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Barrier(ResourceBarrier),
    BeginRenderPass(RenderPassDesc),
    EndRenderPass,
    BindPipeline(PipelineHandle),
    PushConstants {
        pipeline: PipelineHandle,
        offset: u32,
        data: Vec<u8>,
    },
    BindGroup {
        set: u32,
        bind_group: BindGroupHandle,
        dynamic_offsets: Vec<u32>,
    },
    CopyTexture {
        source: TextureHandle,
        destination: TextureHandle,
    },
    BlitTexture {
        source: TextureHandle,
        destination: TextureHandle,
    },
    BindVertexBuffer {
        slot: u32,
        buffer: BufferHandle,
        offset: u64,
    },
    BindIndexBuffer {
        buffer: BufferHandle,
        offset: u64,
    },
    Draw {
        vertex_count: u32,
        first_vertex: u32,
    },
    DrawIndexed {
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    },
    Dispatch {
        x: u32,
        y: u32,
        z: u32,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandBuffer {
    label: String,
    commands: Vec<Command>,
    native: Option<u64>,
}

impl CommandBuffer {
    pub(crate) fn new(label: String, commands: Vec<Command>) -> Self {
        Self {
            label,
            commands,
            native: None,
        }
    }

    /// Construct a command buffer with an opaque backend token.
    ///
    /// The token is deliberately an integer so the RHI does not expose a
    /// backend-specific command-buffer type across the crate boundary.
    pub fn with_native(label: String, commands: Vec<Command>, native: Option<u64>) -> Self {
        Self {
            label,
            commands,
            native,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Return the opaque backend token, if one was produced by the backend.
    pub fn native(&self) -> Option<u64> {
        self.native
    }
}

pub trait CommandEncoder: Send {
    fn barrier(&mut self, barrier: ResourceBarrier) -> RhiResult<()>;
    fn begin_render_pass(&mut self, desc: RenderPassDesc) -> RhiResult<()>;
    fn end_render_pass(&mut self) -> RhiResult<()>;
    /// Copy a color texture into another color texture.
    ///
    /// Callers are responsible for transitioning the source to `CopySrc` and
    /// the destination to `CopyDst` before issuing the copy, then restoring
    /// the states required by subsequent work.
    fn copy_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()>;
    /// Copy a color texture while allowing the backend to scale and convert
    /// between compatible color formats. This is the RHI fallback used by the
    /// core tone-mapping boundary until a real post-process pipeline is bound.
    fn blit_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()>;
    fn bind_pipeline(&mut self, pipeline: PipelineHandle) -> RhiResult<()>;
    fn push_constants(
        &mut self,
        pipeline: PipelineHandle,
        offset: u32,
        data: &[u8],
    ) -> RhiResult<()>;
    fn bind_group(
        &mut self,
        set: u32,
        bind_group: BindGroupHandle,
        dynamic_offsets: &[u32],
    ) -> RhiResult<()>;
    fn bind_vertex_buffer(&mut self, slot: u32, buffer: BufferHandle, offset: u64)
        -> RhiResult<()>;
    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64) -> RhiResult<()>;
    fn draw(&mut self, vertex_count: u32, first_vertex: u32) -> RhiResult<()>;
    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> RhiResult<()>;
    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> RhiResult<()>;
    fn finish(self: Box<Self>) -> RhiResult<CommandBuffer>;
}

#[derive(Debug)]
pub struct RecordingCommandEncoder {
    label: String,
    commands: Vec<Command>,
    render_pass_open: bool,
    pipeline_bound: bool,
}

impl RecordingCommandEncoder {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            commands: Vec::new(),
            render_pass_open: false,
            pipeline_bound: false,
        }
    }

    fn require_render_pass(&self) -> RhiResult<()> {
        if self.render_pass_open {
            Ok(())
        } else {
            Err(RhiError::Message(String::from(
                "draw commands require an open render pass",
            )))
        }
    }

    fn require_no_render_pass(&self) -> RhiResult<()> {
        if self.render_pass_open {
            Err(RhiError::Message(String::from(
                "the command is not valid inside a render pass",
            )))
        } else {
            Ok(())
        }
    }
}

impl CommandEncoder for RecordingCommandEncoder {
    fn barrier(&mut self, barrier: ResourceBarrier) -> RhiResult<()> {
        self.commands.push(Command::Barrier(barrier));
        Ok(())
    }

    fn begin_render_pass(&mut self, desc: RenderPassDesc) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "a render pass is already open",
            )));
        }
        if desc.colors.is_empty() && desc.depth.is_none() {
            return Err(RhiError::InvalidDescription(
                "render pass must contain an attachment",
            ));
        }
        if desc
            .colors
            .iter()
            .any(|attachment| !attachment.texture.is_valid())
            || desc
                .depth
                .is_some_and(|attachment| !attachment.texture.is_valid())
        {
            return Err(RhiError::InvalidHandle);
        }
        self.render_pass_open = true;
        self.commands.push(Command::BeginRenderPass(desc));
        Ok(())
    }

    fn end_render_pass(&mut self) -> RhiResult<()> {
        if !self.render_pass_open {
            return Err(RhiError::Message(String::from("no render pass is open")));
        }
        self.render_pass_open = false;
        self.commands.push(Command::EndRenderPass);
        Ok(())
    }

    fn copy_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()> {
        self.require_no_render_pass()?;
        if !source.is_valid() || !destination.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        self.commands.push(Command::CopyTexture {
            source,
            destination,
        });
        Ok(())
    }

    fn blit_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()> {
        self.require_no_render_pass()?;
        if !source.is_valid() || !destination.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        self.commands.push(Command::BlitTexture {
            source,
            destination,
        });
        Ok(())
    }

    fn bind_pipeline(&mut self, pipeline: PipelineHandle) -> RhiResult<()> {
        if !pipeline.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        self.pipeline_bound = true;
        self.commands.push(Command::BindPipeline(pipeline));
        Ok(())
    }

    fn push_constants(
        &mut self,
        pipeline: PipelineHandle,
        offset: u32,
        data: &[u8],
    ) -> RhiResult<()> {
        if !self.pipeline_bound || !pipeline.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        if data.is_empty() || offset % 4 != 0 || data.len() % 4 != 0 {
            return Err(RhiError::InvalidDescription(
                "push constants must be non-empty and four-byte aligned",
            ));
        }
        self.commands.push(Command::PushConstants {
            pipeline,
            offset,
            data: data.to_vec(),
        });
        Ok(())
    }

    fn bind_group(
        &mut self,
        set: u32,
        bind_group: BindGroupHandle,
        dynamic_offsets: &[u32],
    ) -> RhiResult<()> {
        if !self.pipeline_bound {
            return Err(RhiError::Message(String::from(
                "a pipeline must be bound before a bind group",
            )));
        }
        if !bind_group.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        self.commands.push(Command::BindGroup {
            set,
            bind_group,
            dynamic_offsets: dynamic_offsets.to_vec(),
        });
        Ok(())
    }

    fn bind_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: BufferHandle,
        offset: u64,
    ) -> RhiResult<()> {
        self.require_render_pass()?;
        self.commands.push(Command::BindVertexBuffer {
            slot,
            buffer,
            offset,
        });
        Ok(())
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64) -> RhiResult<()> {
        self.require_render_pass()?;
        self.commands
            .push(Command::BindIndexBuffer { buffer, offset });
        Ok(())
    }

    fn draw(&mut self, vertex_count: u32, first_vertex: u32) -> RhiResult<()> {
        self.require_render_pass()?;
        self.commands.push(Command::Draw {
            vertex_count,
            first_vertex,
        });
        Ok(())
    }

    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> RhiResult<()> {
        self.require_render_pass()?;
        self.commands.push(Command::DrawIndexed {
            index_count,
            instance_count,
            first_index,
            vertex_offset,
            first_instance,
        });
        Ok(())
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> RhiResult<()> {
        self.require_no_render_pass()?;
        self.commands.push(Command::Dispatch { x, y, z });
        Ok(())
    }

    fn finish(self: Box<Self>) -> RhiResult<CommandBuffer> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "cannot finish while a render pass is open",
            )));
        }
        Ok(CommandBuffer::new(self.label, self.commands))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhi::types::TextureHandle;

    #[test]
    fn render_pass_requires_an_attachment() {
        let mut encoder = RecordingCommandEncoder::new("test");
        assert!(matches!(
            encoder.begin_render_pass(RenderPassDesc::default()),
            Err(RhiError::InvalidDescription(_))
        ));
    }

    #[test]
    fn dispatch_is_outside_render_pass() {
        let mut encoder = RecordingCommandEncoder::new("test");
        encoder
            .begin_render_pass(RenderPassDesc {
                colors: vec![RenderAttachment {
                    texture: TextureHandle::new(0, 1),
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                    clear: ClearValue::default(),
                }],
                ..RenderPassDesc::default()
            })
            .unwrap();
        assert!(encoder.dispatch(1, 1, 1).is_err());
    }

    #[test]
    fn blit_is_recorded_outside_render_pass() {
        let mut encoder = RecordingCommandEncoder::new("test");
        encoder
            .blit_texture(TextureHandle::new(1, 1), TextureHandle::new(2, 1))
            .unwrap();
        assert_eq!(
            encoder.commands.last(),
            Some(&Command::BlitTexture {
                source: TextureHandle::new(1, 1),
                destination: TextureHandle::new(2, 1),
            })
        );
    }
}
