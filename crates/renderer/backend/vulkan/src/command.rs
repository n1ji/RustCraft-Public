use std::collections::HashMap;
use std::sync::Arc;

use ash::vk;
use ash::vk::Handle;

use renderer::rhi::command::*;
use renderer::rhi::types::*;

use super::context::NativeResourceTables;
use super::resources::texture_aspect;

pub(crate) fn image_barrier_state(
    state: ResourceState,
) -> (vk::PipelineStageFlags, vk::AccessFlags, vk::ImageLayout) {
    match state {
        ResourceState::Undefined => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
            vk::ImageLayout::UNDEFINED,
        ),
        ResourceState::CopySrc => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_READ,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        ),
        ResourceState::CopyDst => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        ),
        ResourceState::ShaderRead => (
            vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::AccessFlags::SHADER_READ,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        ),
        ResourceState::StorageRead => (
            vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::SHADER_READ,
            vk::ImageLayout::GENERAL,
        ),
        ResourceState::StorageWrite => (
            vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::SHADER_WRITE,
            vk::ImageLayout::GENERAL,
        ),
        ResourceState::ColorAttachment => (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        ),
        ResourceState::DepthAttachment => (
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        ),
        ResourceState::Present => (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
            vk::ImageLayout::PRESENT_SRC_KHR,
        ),
        ResourceState::VertexBuffer | ResourceState::IndexBuffer | ResourceState::UniformBuffer => {
            (
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::AccessFlags::empty(),
                vk::ImageLayout::GENERAL,
            )
        }
    }
}

fn image_layout_state(layout: vk::ImageLayout) -> (vk::PipelineStageFlags, vk::AccessFlags) {
    match layout {
        vk::ImageLayout::UNDEFINED => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_READ,
        ),
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_WRITE,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::AccessFlags::SHADER_READ,
        ),
        vk::ImageLayout::GENERAL => (
            vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::PRESENT_SRC_KHR => (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        _ => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
    }
}

fn buffer_barrier_state(state: ResourceState) -> (vk::PipelineStageFlags, vk::AccessFlags) {
    match state {
        ResourceState::Undefined => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        ResourceState::CopySrc => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_READ,
        ),
        ResourceState::CopyDst => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_WRITE,
        ),
        ResourceState::VertexBuffer => (
            vk::PipelineStageFlags::VERTEX_INPUT,
            vk::AccessFlags::VERTEX_ATTRIBUTE_READ,
        ),
        ResourceState::IndexBuffer => (
            vk::PipelineStageFlags::VERTEX_INPUT,
            vk::AccessFlags::INDEX_READ,
        ),
        ResourceState::UniformBuffer => (
            vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::UNIFORM_READ,
        ),
        ResourceState::StorageRead => (
            vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::SHADER_READ,
        ),
        ResourceState::StorageWrite => (
            vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::SHADER_WRITE,
        ),
        ResourceState::ShaderRead
        | ResourceState::ColorAttachment
        | ResourceState::DepthAttachment
        | ResourceState::Present => (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
    }
}

fn vulkan_load_op(load: LoadOp) -> vk::AttachmentLoadOp {
    match load {
        LoadOp::Load => vk::AttachmentLoadOp::LOAD,
        LoadOp::Clear => vk::AttachmentLoadOp::CLEAR,
        LoadOp::DontCare => vk::AttachmentLoadOp::DONT_CARE,
    }
}

fn vulkan_store_op(store: StoreOp) -> vk::AttachmentStoreOp {
    match store {
        StoreOp::Store => vk::AttachmentStoreOp::STORE,
        StoreOp::DontCare => vk::AttachmentStoreOp::DONT_CARE,
    }
}

fn vulkan_clear_value(clear: ClearValue) -> vk::ClearValue {
    match clear {
        ClearValue::Color(value) => vk::ClearValue {
            color: vk::ClearColorValue { uint32: value },
        },
        ClearValue::DepthStencil { depth, stencil } => vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: f32::from_bits(depth),
                stencil,
            },
        },
    }
}

pub(crate) struct VulkanCommandEncoder {
    pub(crate) device: ash::Device,
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) label: String,
    /// Vulkan records native commands directly. Only barriers are retained
    /// in the RHI log because submit uses them to publish tracked image
    /// layouts; retaining every draw/bind command duplicated the native
    /// command stream and allocated once per packet.
    pub(crate) commands: Vec<Command>,
    pub(crate) render_pass_open: bool,
    pub(crate) bound_pipeline: Option<vk::PipelineBindPoint>,
    pub(crate) bound_pipeline_handle: Option<PipelineHandle>,
    pub(crate) resource_tables: Arc<NativeResourceTables>,
    pub(crate) texture_layouts: HashMap<TextureHandle, vk::ImageLayout>,
}

impl VulkanCommandEncoder {
    fn require_render_pass(&self) -> RhiResult<()> {
        if self.render_pass_open {
            Ok(())
        } else {
            Err(RhiError::Message(String::from(
                "draw commands require an open render pass",
            )))
        }
    }

    fn texture_layout(&self, handle: TextureHandle) -> Option<vk::ImageLayout> {
        self.resource_tables
            .texture_layouts
            .read()
            .ok()?
            .get(&handle)
            .copied()
    }

    fn transition_texture(
        &mut self,
        handle: TextureHandle,
        next_state: ResourceState,
    ) -> RhiResult<()> {
        let Some(&image) = self.resource_tables.texture_images.get(&handle) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(old_layout) = self
            .texture_layouts
            .get(&handle)
            .copied()
            .or_else(|| self.texture_layout(handle))
        else {
            return Err(RhiError::InvalidHandle);
        };
        let (_, _, new_layout) = image_barrier_state(next_state);
        if old_layout == new_layout {
            return Ok(());
        }
        let (src_stage, src_access) = image_layout_state(old_layout);
        let (dst_stage, dst_access, _) = image_barrier_state(next_state);
        let aspect_mask = self
            .resource_tables
            .texture_descs
            .get(&handle)
            .map(|desc| texture_aspect(desc.format, desc.usage))
            .unwrap_or(vk::ImageAspectFlags::COLOR);
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                src_stage,
                dst_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier {
                    src_access_mask: src_access,
                    dst_access_mask: dst_access,
                    old_layout,
                    new_layout,
                    image,
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    ..Default::default()
                }],
            );
        }
        self.texture_layouts.insert(handle, new_layout);
        Ok(())
    }
}

impl CommandEncoder for VulkanCommandEncoder {
    fn barrier(&mut self, barrier: ResourceBarrier) -> RhiResult<()> {
        self.commands.push(Command::Barrier(barrier));
        match barrier.resource {
            ResourceHandle::Texture(handle) => {
                let Some(&image) = self.resource_tables.texture_images.get(&handle) else {
                    return Err(RhiError::InvalidHandle);
                };
                let (_, _, requested_old_layout) = image_barrier_state(barrier.before);
                let actual_old_layout = self
                    .texture_layouts
                    .get(&handle)
                    .copied()
                    .or_else(|| self.texture_layout(handle))
                    .unwrap_or(requested_old_layout);
                let (src_stage, src_access) = if actual_old_layout == requested_old_layout {
                    let (stage, access, _) = image_barrier_state(barrier.before);
                    (stage, access)
                } else {
                    image_layout_state(actual_old_layout)
                };
                let (dst_stage, dst_access, new_layout) = image_barrier_state(barrier.after);
                let aspect_mask = self
                    .resource_tables
                    .texture_descs
                    .get(&handle)
                    .map(|desc| texture_aspect(desc.format, desc.usage))
                    .unwrap_or(vk::ImageAspectFlags::COLOR);
                if actual_old_layout != new_layout {
                    unsafe {
                        self.device.cmd_pipeline_barrier(
                            self.command_buffer,
                            src_stage,
                            dst_stage,
                            vk::DependencyFlags::empty(),
                            &[],
                            &[],
                            &[vk::ImageMemoryBarrier {
                                src_access_mask: src_access,
                                dst_access_mask: dst_access,
                                old_layout: actual_old_layout,
                                new_layout,
                                image,
                                subresource_range: vk::ImageSubresourceRange {
                                    aspect_mask,
                                    base_mip_level: 0,
                                    level_count: 1,
                                    base_array_layer: 0,
                                    layer_count: 1,
                                },
                                ..Default::default()
                            }],
                        );
                    }
                }
                self.texture_layouts.insert(handle, new_layout);
            }
            ResourceHandle::Buffer(handle) => {
                let Some(&buffer) = self.resource_tables.buffer_objects.get(&handle) else {
                    return Err(RhiError::InvalidHandle);
                };
                let (src_stage, src_access) = buffer_barrier_state(barrier.before);
                let (dst_stage, dst_access) = buffer_barrier_state(barrier.after);
                unsafe {
                    self.device.cmd_pipeline_barrier(
                        self.command_buffer,
                        src_stage,
                        dst_stage,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[vk::BufferMemoryBarrier {
                            src_access_mask: src_access,
                            dst_access_mask: dst_access,
                            buffer,
                            offset: 0,
                            size: vk::WHOLE_SIZE,
                            ..Default::default()
                        }],
                        &[],
                    );
                }
            }
            ResourceHandle::Shader(_)
            | ResourceHandle::Pipeline(_)
            | ResourceHandle::Sampler(_)
            | ResourceHandle::BindGroup(_)
            | ResourceHandle::RenderTarget(_) => {}
        }
        Ok(())
    }

    fn begin_render_pass(&mut self, desc: RenderPassDesc) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "a render pass is already open",
            )));
        }
        for attachment in &desc.colors {
            self.transition_texture(attachment.texture, ResourceState::ColorAttachment)?;
        }
        if let Some(attachment) = desc.depth {
            self.transition_texture(attachment.texture, ResourceState::DepthAttachment)?;
        }
        let mut extent = None;
        for attachment in desc.colors.iter().chain(desc.depth.iter()) {
            let Some(texture_desc) = self.resource_tables.texture_descs.get(&attachment.texture)
            else {
                return Err(RhiError::InvalidHandle);
            };
            let next_extent = (texture_desc.extent.width, texture_desc.extent.height);
            if let Some(previous_extent) = extent {
                if previous_extent != next_extent {
                    return Err(RhiError::InvalidDescription(
                        "render pass attachments must have matching extents",
                    ));
                }
            } else {
                extent = Some(next_extent);
            }
        }
        let Some((width, height)) = extent else {
            return Err(RhiError::InvalidDescription(
                "render pass must contain an attachment",
            ));
        };
        let mut colors = Vec::with_capacity(desc.colors.len());
        for attachment in &desc.colors {
            let Some(&view) = self.resource_tables.texture_views.get(&attachment.texture) else {
                return Err(RhiError::InvalidHandle);
            };
            colors.push(
                vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(vulkan_load_op(attachment.load))
                    .store_op(vulkan_store_op(attachment.store))
                    .clear_value(vulkan_clear_value(attachment.clear)),
            );
        }
        let depth = if let Some(attachment) = desc.depth {
            let Some(&view) = self.resource_tables.texture_views.get(&attachment.texture) else {
                return Err(RhiError::InvalidHandle);
            };
            Some(
                vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .load_op(vulkan_load_op(attachment.load))
                    .store_op(vulkan_store_op(attachment.store))
                    .clear_value(vulkan_clear_value(attachment.clear)),
            )
        } else {
            None
        };
        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width, height },
        };
        let mut info = vk::RenderingInfo::default()
            .render_area(render_area)
            .layer_count(1)
            .color_attachments(&colors);
        if let Some(depth) = depth.as_ref() {
            info = info.depth_attachment(depth);
        }
        unsafe {
            self.device.cmd_begin_rendering(self.command_buffer, &info);
            self.device.cmd_set_viewport(
                self.command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: width as f32,
                    height: height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            self.device
                .cmd_set_scissor(self.command_buffer, 0, &[render_area]);
        }
        self.render_pass_open = true;
        Ok(())
    }

    fn end_render_pass(&mut self) -> RhiResult<()> {
        if !self.render_pass_open {
            return Err(RhiError::Message(String::from("no render pass is open")));
        }
        unsafe { self.device.cmd_end_rendering(self.command_buffer) };
        self.render_pass_open = false;
        Ok(())
    }

    fn copy_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "texture copies cannot be recorded inside a render pass",
            )));
        }
        let Some(&source_image) = self.resource_tables.texture_images.get(&source) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(&destination_image) = self.resource_tables.texture_images.get(&destination) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(source_desc) = self.resource_tables.texture_descs.get(&source) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(destination_desc) = self.resource_tables.texture_descs.get(&destination) else {
            return Err(RhiError::InvalidHandle);
        };
        if source_desc.extent != destination_desc.extent {
            return Err(RhiError::InvalidDescription(
                "texture copy extents must match",
            ));
        }
        if source_desc.format != destination_desc.format {
            return Err(RhiError::InvalidDescription(
                "texture copy formats must match",
            ));
        }
        let source_aspect = texture_aspect(source_desc.format, source_desc.usage);
        let destination_aspect = texture_aspect(destination_desc.format, destination_desc.usage);
        if source_aspect != vk::ImageAspectFlags::COLOR
            || destination_aspect != vk::ImageAspectFlags::COLOR
        {
            return Err(RhiError::InvalidDescription(
                "only color textures can be copied",
            ));
        }
        unsafe {
            self.device.cmd_copy_image(
                self.command_buffer,
                source_image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                destination_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy {
                    src_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: source_aspect,
                        layer_count: 1,
                        ..Default::default()
                    },
                    dst_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: destination_aspect,
                        layer_count: 1,
                        ..Default::default()
                    },
                    extent: vk::Extent3D {
                        width: source_desc.extent.width,
                        height: source_desc.extent.height,
                        depth: 1,
                    },
                    ..Default::default()
                }],
            );
        }
        Ok(())
    }

    fn copy_buffer(
        &mut self,
        source: BufferHandle,
        destination: BufferHandle,
        source_offset: u64,
        destination_offset: u64,
        size: u64,
    ) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "buffer copies cannot be recorded inside a render pass",
            )));
        }
        let Some(&source_buffer) = self.resource_tables.buffer_objects.get(&source) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(&destination_buffer) = self.resource_tables.buffer_objects.get(&destination)
        else {
            return Err(RhiError::InvalidHandle);
        };
        let source_size = self
            .resource_tables
            .buffer_sizes
            .get(&source)
            .copied()
            .ok_or(RhiError::InvalidHandle)?;
        let destination_size = self
            .resource_tables
            .buffer_sizes
            .get(&destination)
            .copied()
            .ok_or(RhiError::InvalidHandle)?;
        let source_end = source_offset
            .checked_add(size)
            .ok_or(RhiError::InvalidDescription("buffer copy range overflow"))?;
        let destination_end = destination_offset
            .checked_add(size)
            .ok_or(RhiError::InvalidDescription("buffer copy range overflow"))?;
        if size == 0 || source_end > source_size || destination_end > destination_size {
            return Err(RhiError::InvalidDescription(
                "buffer copy range exceeds a buffer",
            ));
        }
        unsafe {
            self.device.cmd_copy_buffer(
                self.command_buffer,
                source_buffer,
                destination_buffer,
                &[vk::BufferCopy {
                    src_offset: source_offset,
                    dst_offset: destination_offset,
                    size,
                }],
            );
            // The upload path records this copy immediately before the buffer
            // is consumed by vertex/index/uniform reads. A conservative
            // transfer-to-memory-read barrier keeps the helper independent of
            // the destination buffer's eventual shader usage.
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[vk::BufferMemoryBarrier {
                    src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                    dst_access_mask: vk::AccessFlags::MEMORY_READ,
                    buffer: destination_buffer,
                    offset: destination_offset,
                    size,
                    ..Default::default()
                }],
                &[],
            );
        }
        self.commands.push(Command::CopyBuffer {
            source,
            destination,
            source_offset,
            destination_offset,
            size,
        });
        Ok(())
    }

    fn blit_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "texture blits cannot be recorded inside a render pass",
            )));
        }
        let Some(&source_image) = self.resource_tables.texture_images.get(&source) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(&destination_image) = self.resource_tables.texture_images.get(&destination) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(source_desc) = self.resource_tables.texture_descs.get(&source) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(destination_desc) = self.resource_tables.texture_descs.get(&destination) else {
            return Err(RhiError::InvalidHandle);
        };
        let source_aspect = texture_aspect(source_desc.format, source_desc.usage);
        let destination_aspect = texture_aspect(destination_desc.format, destination_desc.usage);
        if source_aspect != vk::ImageAspectFlags::COLOR
            || destination_aspect != vk::ImageAspectFlags::COLOR
        {
            return Err(RhiError::InvalidDescription(
                "only color textures can be blitted",
            ));
        }
        unsafe {
            self.device.cmd_blit_image(
                self.command_buffer,
                source_image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                destination_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageBlit {
                    src_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: source_aspect,
                        layer_count: 1,
                        ..Default::default()
                    },
                    src_offsets: [
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                            x: source_desc.extent.width as i32,
                            y: source_desc.extent.height as i32,
                            z: 1,
                        },
                    ],
                    dst_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: destination_aspect,
                        layer_count: 1,
                        ..Default::default()
                    },
                    dst_offsets: [
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                            x: destination_desc.extent.width as i32,
                            y: destination_desc.extent.height as i32,
                            z: 1,
                        },
                    ],
                }],
                vk::Filter::LINEAR,
            );
        }
        Ok(())
    }

    fn bind_pipeline(&mut self, pipeline: PipelineHandle) -> RhiResult<()> {
        let Some(native_pipeline) = self.resource_tables.pipelines.get(&pipeline).cloned() else {
            return Err(RhiError::InvalidHandle);
        };
        unsafe {
            self.device.cmd_bind_pipeline(
                self.command_buffer,
                native_pipeline.bind_point,
                native_pipeline.pipeline,
            );
        }
        self.bound_pipeline = Some(native_pipeline.bind_point);
        self.bound_pipeline_handle = Some(pipeline);
        Ok(())
    }

    fn push_constants(
        &mut self,
        pipeline: PipelineHandle,
        offset: u32,
        data: &[u8],
    ) -> RhiResult<()> {
        if self.bound_pipeline_handle != Some(pipeline) {
            return Err(RhiError::InvalidHandle);
        }
        if data.is_empty() || offset % 4 != 0 || data.len() % 4 != 0 {
            return Err(RhiError::InvalidDescription(
                "push constants must be non-empty and four-byte aligned",
            ));
        }
        let Some(native_pipeline) = self.resource_tables.pipelines.get(&pipeline) else {
            return Err(RhiError::InvalidHandle);
        };
        unsafe {
            self.device.cmd_push_constants(
                self.command_buffer,
                native_pipeline.layout,
                vk::ShaderStageFlags::ALL_GRAPHICS | vk::ShaderStageFlags::COMPUTE,
                offset,
                data,
            );
        }
        Ok(())
    }

    fn bind_group(
        &mut self,
        set: u32,
        bind_group: BindGroupHandle,
        dynamic_offsets: &[u32],
    ) -> RhiResult<()> {
        let Some(group) = self.resource_tables.bind_groups.get(&bind_group) else {
            return Err(RhiError::InvalidHandle);
        };
        if group.set != set || self.bound_pipeline_handle != Some(group.pipeline) {
            return Err(RhiError::InvalidDescription(
                "bind group does not match the bound pipeline set",
            ));
        }
        let Some(pipeline) = self.resource_tables.pipelines.get(&group.pipeline) else {
            return Err(RhiError::InvalidHandle);
        };
        unsafe {
            self.device.cmd_bind_descriptor_sets(
                self.command_buffer,
                pipeline.bind_point,
                pipeline.layout,
                set,
                &[group.descriptor_set],
                dynamic_offsets,
            );
        }
        Ok(())
    }

    fn bind_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: BufferHandle,
        offset: u64,
    ) -> RhiResult<()> {
        self.require_render_pass()?;
        let Some(&native_buffer) = self.resource_tables.buffer_objects.get(&buffer) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(&buffer_size) = self.resource_tables.buffer_sizes.get(&buffer) else {
            return Err(RhiError::InvalidHandle);
        };
        if offset > buffer_size {
            return Err(RhiError::InvalidDescription(
                "vertex buffer offset exceeds the buffer",
            ));
        }
        unsafe {
            self.device.cmd_bind_vertex_buffers(
                self.command_buffer,
                slot,
                &[native_buffer],
                &[offset],
            );
        }
        Ok(())
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64) -> RhiResult<()> {
        self.require_render_pass()?;
        let Some(&native_buffer) = self.resource_tables.buffer_objects.get(&buffer) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(&buffer_size) = self.resource_tables.buffer_sizes.get(&buffer) else {
            return Err(RhiError::InvalidHandle);
        };
        if offset > buffer_size {
            return Err(RhiError::InvalidDescription(
                "index buffer offset exceeds the buffer",
            ));
        }
        unsafe {
            self.device.cmd_bind_index_buffer(
                self.command_buffer,
                native_buffer,
                offset,
                vk::IndexType::UINT32,
            );
        }
        Ok(())
    }

    fn draw(&mut self, vertex_count: u32, first_vertex: u32) -> RhiResult<()> {
        self.require_render_pass()?;
        if self.bound_pipeline != Some(vk::PipelineBindPoint::GRAPHICS) {
            return Err(RhiError::Message(String::from(
                "draw requires a graphics pipeline",
            )));
        }
        unsafe {
            self.device
                .cmd_draw(self.command_buffer, vertex_count, 1, first_vertex, 0)
        };
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
        if self.bound_pipeline != Some(vk::PipelineBindPoint::GRAPHICS) {
            return Err(RhiError::Message(String::from(
                "indexed draw requires a graphics pipeline",
            )));
        }
        unsafe {
            self.device.cmd_draw_indexed(
                self.command_buffer,
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
        Ok(())
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "dispatch cannot be recorded inside a render pass",
            )));
        }
        if self.bound_pipeline != Some(vk::PipelineBindPoint::COMPUTE) {
            return Err(RhiError::Message(String::from(
                "dispatch requires a compute pipeline",
            )));
        }
        unsafe { self.device.cmd_dispatch(self.command_buffer, x, y, z) };
        Ok(())
    }

    fn finish(self: Box<Self>) -> RhiResult<CommandBuffer> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "cannot finish while a render pass is open",
            )));
        }
        unsafe {
            self.device
                .end_command_buffer(self.command_buffer)
                .map_err(|error| {
                    RhiError::Backend(format!("end Vulkan command buffer: {error:?}"))
                })?;
        }
        Ok(CommandBuffer::with_native(
            self.label,
            self.commands,
            Some(self.command_buffer.as_raw()),
        ))
    }
}
